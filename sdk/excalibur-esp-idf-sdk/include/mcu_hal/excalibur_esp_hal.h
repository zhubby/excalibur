#ifndef EXCALIBUR_ESP_HAL_H
#define EXCALIBUR_ESP_HAL_H

#include "esp_log.h"
#include "excalibur_client.h"

typedef enum excalibur_reset_reason {
    EXCALIBUR_RST_UNKNOWN,
    EXCALIBUR_RST_POWERON,
    EXCALIBUR_RST_EXT,
    EXCALIBUR_RST_SW,
    EXCALIBUR_RST_PANIC,
    EXCALIBUR_RST_INT_WDT,
    EXCALIBUR_RST_TASK_WDT,
    EXCALIBUR_RST_WDT,
    EXCALIBUR_RST_DEEPSLEEP,
    EXCALIBUR_RST_BROWNOUT,
    EXCALIBUR_RST_SDIO,
} excalibur_reset_reason_t;

#define EXCALIBUR_LOGE(tag, fmt, ...) ESP_LOGE(tag, fmt, ##__VA_ARGS__)
#define EXCALIBUR_LOGW(tag, fmt, ...) ESP_LOGW(tag, fmt, ##__VA_ARGS__)
#define EXCALIBUR_LOGI(tag, fmt, ...) ESP_LOGI(tag, fmt, ##__VA_ARGS__)
#define EXCALIBUR_LOGD(tag, fmt, ...) ESP_LOGD(tag, fmt, ##__VA_ARGS__)
#define EXCALIBUR_LOGV(tag, fmt, ...) ESP_LOGV(tag, fmt, ##__VA_ARGS__)

int excalibur_hal_mqtt_subscribe(excalibur_client_handle_t client, char *topic, int qos);
int excalibur_hal_mqtt_unsubscribe(excalibur_client_handle_t client, char *topic);
int excalibur_hal_mqtt_publish(excalibur_client_handle_t client, char *topic, char *message, int length, int qos);
int excalibur_hal_restart(void);
int excalibur_hal_ota(excalibur_client_t *excalibur_client, char *ota_url);
int excalibur_hal_init(excalibur_client_t *excalibur_client);
int excalibur_hal_destroy(excalibur_client_t *excalibur_client);
int excalibur_hal_start_mqtt(excalibur_client_t *excalibur_client);
int excalibur_hal_stop_mqtt(excalibur_client_t *excalibur_client);
int excalibur_hal_spiffs_mount(void);
int excalibur_hal_spiffs_unmount(void);
int excalibur_hal_fatfs_mount(void);
int excalibur_hal_fatfs_unmount(void);
unsigned long long excalibur_hal_get_epoch_millis(void);
excalibur_reset_reason_t excalibur_hal_get_reset_reason(void);
long long excalibur_hal_get_uptime_ms(void);

#endif
