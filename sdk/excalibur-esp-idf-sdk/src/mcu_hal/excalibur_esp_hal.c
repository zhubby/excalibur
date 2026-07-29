#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include "nvs.h"
#include "nvs_flash.h"
#include "esp_event.h"
#include "esp_http_client.h"
#include "esp_https_ota.h"
#include "esp_idf_version.h"
#include "esp_ota_ops.h"
#include "esp_spiffs.h"
#include "esp_system.h"
#include "esp_timer.h"
#include "esp_vfs_fat.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "mqtt_client.h"
#include "wear_levelling.h"
#include "excalibur_esp_hal.h"
#include "excalibur_ota.h"
#include "excalibur_action.h"
#include "excalibur_stream.h"

static int ota_img_data_len = 0;
static int ota_update_completed = 0;
static char ota_action_id_str[EXCALIBUR_ACTION_ID_STR_LEN] = "";
static excalibur_client_t *ota_client = NULL;

static const char *TAG = "EXCALIBUR_HAL";

int excalibur_hal_mqtt_subscribe(excalibur_client_handle_t client, char *topic, int qos)
{
    return esp_mqtt_client_subscribe(client, (const char *)topic, qos);
}

int excalibur_hal_mqtt_unsubscribe(excalibur_client_handle_t client, char *topic)
{
    return esp_mqtt_client_unsubscribe(client, (const char *)topic);
}

int excalibur_hal_mqtt_publish(excalibur_client_handle_t client, char *topic, char *message, int length, int qos)
{
    return esp_mqtt_client_publish(client, (const char *)topic, (const char *)message, length, qos, 1);
}

int excalibur_hal_restart(void)
{
    esp_restart();
    return 0;
}

esp_err_t _http_event_handler(esp_http_client_event_t *evt)
{
    static int next_progress = 0;
    static int downloaded_data_len = 0;

    if (evt->event_id == HTTP_EVENT_ON_DATA) {
        downloaded_data_len += evt->data_len;
        int update_progress_percent = 0;
        if (ota_img_data_len > 0) {
            update_progress_percent = (((float)downloaded_data_len / (float)ota_img_data_len) * 100.0f);
        }

        if (update_progress_percent >= next_progress) {
            int reported_progress = update_progress_percent >= 100 ? 99 : update_progress_percent;
            excalibur_publish_action_status(ota_client, excalibur_ota_action_id, reported_progress, EXCALIBUR_COMMAND_RUNNING, "");
            next_progress += 10;
        }

        if (update_progress_percent >= 100) {
            next_progress = 0;
            downloaded_data_len = 0;
        }
    }

    return ESP_OK;
}

esp_err_t _test_event_handler(esp_http_client_event_t *evt)
{
    if (evt->event_id == HTTP_EVENT_ON_DATA) {
        ota_img_data_len += evt->data_len;
    }
    return ESP_OK;
}

int excalibur_hal_ota(excalibur_client_t *excalibur_client, char *ota_url)
{
    ota_client = excalibur_client;
    esp_http_client_config_t config = {
        .url = ota_url,
        .cert_pem = (char *)ota_client->device_cfg.ca_cert_pem,
        .client_cert_pem = (char *)ota_client->device_cfg.client_cert_pem,
        .client_key_pem = (char *)ota_client->device_cfg.client_key_pem,
        .event_handler = _http_event_handler,
    };
    esp_http_client_config_t test_config = {
        .url = ota_url,
        .cert_pem = (char *)ota_client->device_cfg.ca_cert_pem,
        .client_cert_pem = (char *)ota_client->device_cfg.client_cert_pem,
        .client_key_pem = (char *)ota_client->device_cfg.client_key_pem,
        .event_handler = _test_event_handler,
    };

#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
    esp_https_ota_config_t ota_config = {
        .http_config = &config,
    };
#endif

    ota_img_data_len = 0;
    esp_http_client_handle_t client = esp_http_client_init(&test_config);
    esp_err_t err = esp_http_client_perform(client);
    if (err != ESP_OK) {
        esp_http_client_cleanup(client);
        return -1;
    }
    esp_http_client_cleanup(client);

#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
    err = esp_https_ota(&ota_config);
#else
    err = esp_https_ota(&config);
#endif
    if (err != ESP_OK) {
        snprintf(excalibur_ota_error_str, EXCALIBUR_OTA_ERROR_STR_LEN, "Error (%d): %s", err, esp_err_to_name(err));
        return -1;
    }

    return 0;
}

static void log_error_if_nonzero(const char *message, int error_code)
{
    if (error_code != 0) {
        EXCALIBUR_LOGE(TAG, "last error %s: 0x%x", message, error_code);
    }
}

static void mqtt_event_handler(void *handler_args, esp_event_base_t base, int32_t event_id, void *event_data)
{
    EXCALIBUR_LOGD(TAG, "event dispatched from %s, event_id=%d", base, (int)event_id);

    esp_mqtt_event_handle_t event = event_data;
    excalibur_client_t *excalibur_client = handler_args;

    switch ((esp_mqtt_event_id_t)event_id) {
    case MQTT_EVENT_CONNECTED: {
        int msg_id = excalibur_subscribe_to_commands(excalibur_client->device_cfg, event->client);
        if (msg_id != -1) {
            EXCALIBUR_LOGI(TAG, "subscribed to commands, msg_id=%d", msg_id);
        } else {
            EXCALIBUR_LOGE(TAG, "command subscription failed");
        }
        excalibur_client->connection_status = 1;
        if (ota_update_completed == 1) {
            ota_update_completed = 0;
            excalibur_publish_action_completed(excalibur_client, ota_action_id_str);
            ota_action_id_str[0] = '\0';
        }
        break;
    }
    case MQTT_EVENT_DISCONNECTED:
        excalibur_client->connection_status = 0;
        break;
    case MQTT_EVENT_DATA: {
        char *payload = malloc(event->data_len + 1);
        if (payload == NULL) {
            EXCALIBUR_LOGE(TAG, "failed to allocate MQTT payload buffer");
            break;
        }
        memcpy(payload, event->data, event->data_len);
        payload[event->data_len] = '\0';
        if (excalibur_handle_command(payload, excalibur_client) != 0) {
            EXCALIBUR_LOGE(TAG, "command handling failed");
        }
        free(payload);
        break;
    }
    case MQTT_EVENT_ERROR:
        if (event->error_handle->error_type == MQTT_ERROR_TYPE_TCP_TRANSPORT) {
            log_error_if_nonzero("reported from esp-tls", event->error_handle->esp_tls_last_esp_err);
            log_error_if_nonzero("reported from tls stack", event->error_handle->esp_tls_stack_err);
            log_error_if_nonzero("captured as transport errno", event->error_handle->esp_transport_sock_errno);
        }
        break;
    default:
        break;
    }
}

int excalibur_hal_init(excalibur_client_t *excalibur_client)
{
    EXCALIBUR_LOGI(TAG, "[APP] free memory: %d bytes", (int)esp_get_free_heap_size());

    excalibur_client->client = esp_mqtt_client_init(&excalibur_client->mqtt_cfg);
    if (excalibur_client->client == NULL) {
        return -1;
    }

    esp_err_t err = esp_mqtt_client_register_event(excalibur_client->client, ESP_EVENT_ANY_ID, mqtt_event_handler, excalibur_client);
    if (err != ESP_OK) {
        return -1;
    }

    err = nvs_flash_init();
    if (err != ESP_OK) {
        return -1;
    }

    nvs_handle_t nvs_handle;
    err = nvs_open("excalibur_ota", NVS_READWRITE, &nvs_handle);
    if (err != ESP_OK) {
        return 0;
    }

    int32_t update_flag = 0;
    err = nvs_get_i32(nvs_handle, "update_flag", &update_flag);
    if (err == ESP_OK && update_flag == 1) {
        update_flag = 0;
        nvs_set_i32(nvs_handle, "update_flag", update_flag);

        size_t action_id_len = sizeof(ota_action_id_str);
        if (nvs_get_str(nvs_handle, "action_id", ota_action_id_str, &action_id_len) == ESP_OK) {
            ota_update_completed = 1;
        }
        nvs_commit(nvs_handle);
    }
    nvs_close(nvs_handle);
    return 0;
}

int excalibur_hal_destroy(excalibur_client_t *excalibur_client)
{
    return esp_mqtt_client_destroy(excalibur_client->client) == ESP_OK ? 0 : -1;
}

int excalibur_hal_start_mqtt(excalibur_client_t *excalibur_client)
{
    esp_err_t err = esp_mqtt_client_start(excalibur_client->client);
    if (err != ESP_OK) {
        return -1;
    }

    xTaskCreate(excalibur_user_thread_entry, "Excalibur Shadow Thread", 4 * 1024, excalibur_client, 2, NULL);
    xTaskCreate(excalibur_mqtt_thread_entry, "Excalibur MQTT Batch Thread", 8 * 1024, NULL, 2, NULL);
    return 0;
}

int excalibur_hal_stop_mqtt(excalibur_client_t *excalibur_client)
{
    return esp_mqtt_client_stop(excalibur_client->client) == ESP_OK ? 0 : -1;
}

int excalibur_hal_spiffs_mount(void)
{
    esp_vfs_spiffs_conf_t conf = {
        .base_path = "/spiffs",
        .partition_label = NULL,
        .max_files = 5,
        .format_if_mount_failed = true
    };
    return esp_vfs_spiffs_register(&conf) == ESP_OK ? 0 : -1;
}

int excalibur_hal_spiffs_unmount(void)
{
    return esp_vfs_spiffs_unregister(NULL) == ESP_OK ? 0 : -1;
}

int excalibur_hal_fatfs_mount(void)
{
    const esp_vfs_fat_mount_config_t conf = {
        .max_files = 4,
        .format_if_mount_failed = false,
        .allocation_unit_size = CONFIG_WL_SECTOR_SIZE
    };
    return esp_vfs_fat_spiflash_mount_ro("/spiflash", "storage", &conf) == ESP_OK ? 0 : -1;
}

int excalibur_hal_fatfs_unmount(void)
{
    return esp_vfs_fat_spiflash_unmount_ro("/spiflash", "storage") == ESP_OK ? 0 : -1;
}

unsigned long long excalibur_hal_get_epoch_millis(void)
{
    struct timeval te;
    gettimeofday(&te, NULL);
    return te.tv_sec * 1000LL + te.tv_usec / 1000;
}

excalibur_reset_reason_t excalibur_hal_get_reset_reason(void)
{
    return (excalibur_reset_reason_t)esp_reset_reason();
}

long long excalibur_hal_get_uptime_ms(void)
{
    return esp_timer_get_time() / 1000;
}
