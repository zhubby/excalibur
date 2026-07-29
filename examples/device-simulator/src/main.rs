use std::time::Duration;

use anyhow::Context;
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use serde_json::json;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let broker = std::env::var("MQTT_BROKER").unwrap_or_else(|_| "localhost".to_owned());
    let port = std::env::var("MQTT_PORT")
        .unwrap_or_else(|_| "1883".to_owned())
        .parse::<u16>()
        .context("MQTT_PORT must be a u16")?;
    let project_id = std::env::var("PROJECT_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());
    let device_id = std::env::var("DEVICE_ID").unwrap_or_else(|_| Uuid::new_v4().to_string());

    let mut options = MqttOptions::new(format!("sim-{device_id}"), broker, port);
    options.set_keep_alive(Duration::from_secs(30));

    let (client, mut eventloop): (AsyncClient, EventLoop) = AsyncClient::new(options, 10);
    let topic = format!("v1/p/{project_id}/d/{device_id}/telemetry/temperature");

    tokio::spawn(async move {
        loop {
            if let Err(error) = eventloop.poll().await {
                eprintln!("mqtt event loop error: {error}");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    });

    let mut sequence = 1;
    loop {
        let payload = json!([
            {
                "sequence": sequence,
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "value": 21.0 + (sequence % 10) as f64 / 10.0,
                "status": "ok"
            }
        ]);
        client.publish(&topic, QoS::AtLeastOnce, false, payload.to_string()).await?;
        sequence += 1;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

