#include <stdio.h>
#include <string.h>
#include "cJSON.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "excalibur_hal.h"
#include "excalibur_stream.h"

static char json_payload[CONFIG_EXCALIBUR_MQTT_BATCH_ELEMENT_SIZE] = "";
static char batch_json_data[CONFIG_EXCALIBUR_NUM_MESSAGES_IN_MQTT_BATCH * CONFIG_EXCALIBUR_MQTT_BATCH_ELEMENT_SIZE] = "";
static char batch_mqtt_stream[64] = "";
static uint64_t batch_sequence = 0;
static excalibur_client_t *excalibur_batch_mqtt_client = NULL;
static SemaphoreHandle_t batch_mqtt_semaphore = NULL;

static const char *TAG = "EXCALIBUR_STREAM";

static int publish_json(excalibur_client_t *excalibur_client, const char *topic, const char *payload)
{
    int msg_id = excalibur_hal_mqtt_publish(excalibur_client->client, (char *)topic, (char *)payload, strlen(payload), 1);
    if (msg_id != -1) {
        EXCALIBUR_LOGI(TAG, "publish successful, msg_id=%d", msg_id);
        return 0;
    }
    return -1;
}

excalibur_err_t excalibur_publish_telemetry(excalibur_client_t *excalibur_client, char *stream_name, uint64_t sequence, char *payload_object_json)
{
    if (excalibur_client == NULL || stream_name == NULL || payload_object_json == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    cJSON *payload_obj = cJSON_Parse(payload_object_json);
    if (!(cJSON_IsObject(payload_obj))) {
        EXCALIBUR_LOGE(TAG, "telemetry payload must be a JSON object");
        cJSON_Delete(payload_obj);
        return EXCALIBUR_FAILURE;
    }

    unsigned long long milliseconds = excalibur_hal_get_epoch_millis();
    cJSON_DeleteItemFromObjectCaseSensitive(payload_obj, "sequence");
    cJSON_DeleteItemFromObjectCaseSensitive(payload_obj, "timestamp");
    cJSON_AddNumberToObject(payload_obj, "sequence", sequence);
    cJSON_AddNumberToObject(payload_obj, "timestamp", milliseconds);

    cJSON *payload_list = cJSON_CreateArray();
    if (payload_list == NULL) {
        cJSON_Delete(payload_obj);
        return EXCALIBUR_FAILURE;
    }
    cJSON_AddItemToArray(payload_list, payload_obj);

    char *string_json = cJSON_PrintUnformatted(payload_list);
    if (string_json == NULL) {
        cJSON_Delete(payload_list);
        return EXCALIBUR_FAILURE;
    }

    char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};
    if (excalibur_build_telemetry_topic(excalibur_client->device_cfg, stream_name, topic, sizeof(topic)) != 0) {
        EXCALIBUR_LOGE(TAG, "telemetry topic size exceeded buffer size");
        cJSON_free(string_json);
        cJSON_Delete(payload_list);
        return EXCALIBUR_FAILURE;
    }

    int ret = publish_json(excalibur_client, topic, string_json);
    cJSON_free(string_json);
    cJSON_Delete(payload_list);
    return ret == 0 ? EXCALIBUR_SUCCESS : EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_publish_shadow(excalibur_client_t *excalibur_client, char *shadow_object_json)
{
    if (excalibur_client == NULL || shadow_object_json == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    cJSON *shadow_obj = cJSON_Parse(shadow_object_json);
    if (!(cJSON_IsObject(shadow_obj))) {
        EXCALIBUR_LOGE(TAG, "shadow payload must be a JSON object");
        cJSON_Delete(shadow_obj);
        return EXCALIBUR_FAILURE;
    }

    char *string_json = cJSON_PrintUnformatted(shadow_obj);
    if (string_json == NULL) {
        cJSON_Delete(shadow_obj);
        return EXCALIBUR_FAILURE;
    }

    char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};
    if (excalibur_build_shadow_topic(excalibur_client->device_cfg, topic, sizeof(topic)) != 0) {
        EXCALIBUR_LOGE(TAG, "shadow topic size exceeded buffer size");
        cJSON_free(string_json);
        cJSON_Delete(shadow_obj);
        return EXCALIBUR_FAILURE;
    }

    int ret = publish_json(excalibur_client, topic, string_json);
    cJSON_free(string_json);
    cJSON_Delete(shadow_obj);
    return ret == 0 ? EXCALIBUR_SUCCESS : EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_publish_device_shadow(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    cJSON *shadow = cJSON_CreateObject();
    if (shadow == NULL) {
        return EXCALIBUR_FAILURE;
    }

    cJSON_AddStringToObject(shadow, "health", CONFIG_EXCALIBUR_SHADOW_STATUS);
    cJSON_AddStringToObject(shadow, "software_type", CONFIG_EXCALIBUR_SHADOW_SOFTWARE_TYPE);
    cJSON_AddStringToObject(shadow, "software_version", CONFIG_EXCALIBUR_SHADOW_SOFTWARE_VERSION);
    cJSON_AddStringToObject(shadow, "hardware_type", CONFIG_EXCALIBUR_SHADOW_HARDWARE_TYPE);
    cJSON_AddStringToObject(shadow, "hardware_version", CONFIG_EXCALIBUR_SHADOW_HARDWARE_VERSION);
    cJSON_AddNumberToObject(shadow, "uptime_ms", excalibur_hal_get_uptime_ms());
    cJSON_AddNumberToObject(shadow, "timestamp", excalibur_hal_get_epoch_millis());

    if (excalibur_client->device_shadow.updater != NULL) {
        excalibur_client->device_shadow.updater(excalibur_client);
    }

    if (excalibur_client->device_shadow.custom_json_str[0] != '\0') {
        cJSON *custom = cJSON_Parse(excalibur_client->device_shadow.custom_json_str);
        if (cJSON_IsObject(custom)) {
            cJSON *child = custom->child;
            while (child != NULL) {
                cJSON_AddItemToObject(shadow, child->string, cJSON_Duplicate(child, true));
                child = child->next;
            }
        }
        cJSON_Delete(custom);
    }

    char *string_json = cJSON_PrintUnformatted(shadow);
    if (string_json == NULL) {
        cJSON_Delete(shadow);
        return EXCALIBUR_FAILURE;
    }

    excalibur_err_t ret = excalibur_publish_shadow(excalibur_client, string_json);
    cJSON_free(string_json);
    cJSON_Delete(shadow);
    return ret;
}

excalibur_err_t excalibur_add_custom_device_shadow(excalibur_client_t *excalibur_client, char *custom_json_str)
{
    if (excalibur_client == NULL || custom_json_str == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    if (strlen(custom_json_str) >= CONFIG_EXCALIBUR_SHADOW_CUSTOM_JSON_STR_LEN) {
        return EXCALIBUR_FAILURE;
    }
    cJSON *custom = cJSON_Parse(custom_json_str);
    if (!(cJSON_IsObject(custom))) {
        cJSON_Delete(custom);
        return EXCALIBUR_FAILURE;
    }
    cJSON_Delete(custom);
    memset(excalibur_client->device_shadow.custom_json_str, 0x00, CONFIG_EXCALIBUR_SHADOW_CUSTOM_JSON_STR_LEN);
    strcpy(excalibur_client->device_shadow.custom_json_str, custom_json_str);
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_register_device_shadow_updater(excalibur_client_t *excalibur_client, int (*func_ptr)(excalibur_client_t *))
{
    if (excalibur_client == NULL || func_ptr == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    excalibur_client->device_shadow.updater = func_ptr;
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_batch_init(excalibur_client_t *excalibur_client, char *stream_name)
{
    if (excalibur_client == NULL || stream_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    batch_mqtt_semaphore = xSemaphoreCreateBinary();
    if (batch_mqtt_semaphore == NULL) {
        return EXCALIBUR_FAILURE;
    }
    excalibur_batch_mqtt_client = excalibur_client;
    snprintf(batch_mqtt_stream, sizeof(batch_mqtt_stream), "%s", stream_name);
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_batch_publish_telemetry(char *payload_object_json)
{
    if (excalibur_batch_mqtt_client == NULL || payload_object_json == NULL || batch_mqtt_semaphore == NULL) {
        return EXCALIBUR_FAILURE;
    }
    if (strlen(payload_object_json) >= sizeof(json_payload)) {
        return EXCALIBUR_FAILURE;
    }
    strcpy(json_payload, payload_object_json);
    xSemaphoreGive(batch_mqtt_semaphore);
    return EXCALIBUR_SUCCESS;
}

void excalibur_user_thread_entry(void *pv)
{
    excalibur_client_t *excalibur_client = (excalibur_client_t *)pv;
    while (1) {
        if (excalibur_publish_device_shadow(excalibur_client) != EXCALIBUR_SUCCESS) {
            EXCALIBUR_LOGE(TAG, "failed to publish device shadow");
        }
        vTaskDelay(CONFIG_EXCALIBUR_SHADOW_PUSH_INTERVAL * 1000 / portTICK_PERIOD_MS);
    }
}

void excalibur_mqtt_thread_entry(void *pv)
{
    (void)pv;
    static int batch_size = 0;

    while (1) {
        if (batch_mqtt_semaphore == NULL) {
            vTaskDelete(NULL);
            return;
        }
        if (batch_mqtt_semaphore != NULL && xSemaphoreTake(batch_mqtt_semaphore, portMAX_DELAY) == pdTRUE) {
            if (batch_size == 0) {
                memset(batch_json_data, 0, sizeof(batch_json_data));
                strcpy(batch_json_data, "[");
            }

            cJSON *payload_obj = cJSON_Parse(json_payload);
            if (!(cJSON_IsObject(payload_obj))) {
                cJSON_Delete(payload_obj);
                batch_size = 0;
                continue;
            }
            cJSON_DeleteItemFromObjectCaseSensitive(payload_obj, "sequence");
            cJSON_DeleteItemFromObjectCaseSensitive(payload_obj, "timestamp");
            cJSON_AddNumberToObject(payload_obj, "sequence", batch_sequence++);
            cJSON_AddNumberToObject(payload_obj, "timestamp", excalibur_hal_get_epoch_millis());
            char *record_json = cJSON_PrintUnformatted(payload_obj);
            cJSON_Delete(payload_obj);
            if (record_json == NULL) {
                batch_size = 0;
                continue;
            }

            batch_size++;
            strcat(batch_json_data, record_json);
            cJSON_free(record_json);

            if (batch_size == CONFIG_EXCALIBUR_NUM_MESSAGES_IN_MQTT_BATCH) {
                strcat(batch_json_data, "]");
                batch_size = 0;

                char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};
                if (excalibur_build_telemetry_topic(excalibur_batch_mqtt_client->device_cfg, batch_mqtt_stream, topic, sizeof(topic)) == 0) {
                    while (publish_json(excalibur_batch_mqtt_client, topic, batch_json_data) != 0) {
                        vTaskDelay(10 / portTICK_PERIOD_MS);
                    }
                }
            } else {
                strcat(batch_json_data, ",");
            }
        }
    }
}
