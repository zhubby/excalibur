use std::time::{Duration, SystemTime, UNIX_EPOCH};

use excalibur_nats_lite::NatsClient;
use serde_json::{Value, json};

#[tokio::test]
async fn live_jetstream_push_consumer_delivers_and_acks_messages() {
    let Ok(nats_url) = std::env::var("EXCALIBUR_NATS_TEST_URL") else {
        eprintln!("skipping live NATS test; EXCALIBUR_NATS_TEST_URL is not set");
        return;
    };
    let unique = unique_name();
    let stream = format!("EXCALIBUR_TEST_{unique}");
    let subject = format!("excalibur.test.{unique}.ingest");
    let delivery_subject = format!("excalibur.test.{unique}.deliver");
    let durable = format!("workers_{unique}");
    let client = NatsClient::new(&nats_url, "excalibur-nats-lite-test").unwrap();

    create_stream(&client, &stream, &subject).await;
    create_consumer(&client, &stream, &durable, &subject, &delivery_subject).await;
    let mut subscription = client
        .subscribe(&delivery_subject, Some(&durable))
        .await
        .unwrap();

    let payload = br#"{"project_id":"p","points":[{"sequence":1}]}"#;
    client.publish(&subject, payload).await.unwrap();

    let message = tokio::time::timeout(Duration::from_secs(5), subscription.next_message())
        .await
        .expect("timed out waiting for JetStream delivery")
        .unwrap();
    assert_eq!(message.payload, payload);
    let ack_subject = message
        .reply
        .expect("JetStream message should include ack subject");
    client.ack(&ack_subject).await.unwrap();

    delete_stream(&client, &stream).await;
}

async fn create_stream(client: &NatsClient, stream: &str, subject: &str) {
    let payload = json!({
        "name": stream,
        "subjects": [subject],
        "retention": "limits",
        "storage": "memory",
    });
    let response = client
        .request(
            &format!("$JS.API.STREAM.CREATE.{stream}"),
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_no_jetstream_error(response.payload);
}

async fn create_consumer(
    client: &NatsClient,
    stream: &str,
    durable: &str,
    subject: &str,
    delivery_subject: &str,
) {
    let payload = json!({
        "stream_name": stream,
        "config": {
            "durable_name": durable,
            "deliver_subject": delivery_subject,
            "deliver_group": durable,
            "filter_subject": subject,
            "deliver_policy": "all",
            "ack_policy": "explicit",
            "max_ack_pending": 16,
        }
    });
    let response = client
        .request(
            &format!("$JS.API.CONSUMER.DURABLE.CREATE.{stream}.{durable}"),
            payload.to_string().as_bytes(),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_no_jetstream_error(response.payload);
}

async fn delete_stream(client: &NatsClient, stream: &str) {
    let _ = client
        .request(
            &format!("$JS.API.STREAM.DELETE.{stream}"),
            b"{}",
            Duration::from_secs(5),
        )
        .await;
}

fn assert_no_jetstream_error(payload: Vec<u8>) {
    let response = serde_json::from_slice::<Value>(&payload).unwrap();
    assert!(
        response.get("error").is_none(),
        "JetStream API returned error: {response}"
    );
}

fn unique_name() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}_{}", std::process::id(), nanos)
}
