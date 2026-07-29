#include <stdio.h>
#include "esp_log.h"
#include "esp_spiffs.h"

static const char *TAG = "EXCALIBUR_PROVISIONING";

void app_main(void)
{
    esp_vfs_spiffs_conf_t conf = {
        .base_path = "/spiffs",
        .partition_label = NULL,
        .max_files = 5,
        .format_if_mount_failed = true
    };

    if (esp_vfs_spiffs_register(&conf) != ESP_OK) {
        ESP_LOGE(TAG, "failed to mount SPIFFS");
        return;
    }

    FILE *file = fopen("/spiffs/device_config.json", "r");
    if (file == NULL) {
        ESP_LOGE(TAG, "device_config.json is not present in SPIFFS");
        esp_vfs_spiffs_unregister(conf.partition_label);
        return;
    }

    fclose(file);
    esp_vfs_spiffs_unregister(conf.partition_label);
    ESP_LOGI(TAG, "Excalibur device provisioning data is present");
}
