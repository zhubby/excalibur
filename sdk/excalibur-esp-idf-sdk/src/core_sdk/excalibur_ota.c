#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cJSON.h"
#include "nvs.h"
#include "nvs_flash.h"
#include "excalibur_hal.h"
#include "excalibur_action.h"
#include "excalibur_ota.h"

char excalibur_ota_action_id[EXCALIBUR_ACTION_ID_STR_LEN] = "";
char excalibur_ota_error_str[EXCALIBUR_OTA_ERROR_STR_LEN] = "";

static const char *TAG = "EXCALIBUR_OTA";

static int is_sha256_hex(const char *value)
{
    if (value == NULL || strlen(value) != 64) {
        return 0;
    }
    for (int i = 0; i < 64; i++) {
        char c = value[i];
        if (!((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F'))) {
            return 0;
        }
    }
    return 1;
}

static int parse_ota_json(char *payload_string, char *url_string_return, size_t url_len)
{
    cJSON *payload_json = cJSON_Parse(payload_string);
    if (payload_json == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to parse OTA JSON");
        return -1;
    }

    cJSON *signed_url = cJSON_GetObjectItem(payload_json, "signed_url");
    cJSON *sha256 = cJSON_GetObjectItem(payload_json, "sha256");
    cJSON *size_bytes = cJSON_GetObjectItem(payload_json, "size_bytes");
    cJSON *component = cJSON_GetObjectItem(payload_json, "component");
    cJSON *version = cJSON_GetObjectItem(payload_json, "version");

    if (!(cJSON_IsString(signed_url) && signed_url->valuestring != NULL) ||
        !(strncmp(signed_url->valuestring, "https://", 8) == 0 || strncmp(signed_url->valuestring, "http://", 7) == 0)) {
        EXCALIBUR_LOGE(TAG, "signed_url must be absolute");
        cJSON_Delete(payload_json);
        return -1;
    }
    if (!(cJSON_IsString(component) && component->valuestring != NULL && component->valuestring[0] != '\0') ||
        !(cJSON_IsString(version) && version->valuestring != NULL && version->valuestring[0] != '\0')) {
        EXCALIBUR_LOGE(TAG, "component and version are required");
        cJSON_Delete(payload_json);
        return -1;
    }
    if (!(cJSON_IsString(sha256) && is_sha256_hex(sha256->valuestring))) {
        EXCALIBUR_LOGE(TAG, "sha256 must be 64 hex characters");
        cJSON_Delete(payload_json);
        return -1;
    }
    if (!(cJSON_IsNumber(size_bytes) && size_bytes->valuedouble > 0)) {
        EXCALIBUR_LOGE(TAG, "size_bytes must be positive");
        cJSON_Delete(payload_json);
        return -1;
    }

    int written = snprintf(url_string_return, url_len, "%s", signed_url->valuestring);
    if (written < 0 || written >= (int)url_len) {
        EXCALIBUR_LOGE(TAG, "OTA URL exceeded buffer size");
        cJSON_Delete(payload_json);
        return -1;
    }

    EXCALIBUR_LOGI(TAG, "installing %s firmware version %s", component->valuestring, version->valuestring);
    cJSON_Delete(payload_json);
    return 0;
}

static int perform_ota(excalibur_client_t *excalibur_client, char *action_id, char *ota_url)
{
    EXCALIBUR_LOGI(TAG, "starting OTA");

    if (excalibur_hal_ota(excalibur_client, ota_url) != -1) {
        esp_err_t err;
        nvs_handle_t nvs_handle;
        int32_t update_flag = 1;

        err = nvs_flash_init();
        if (err != ESP_OK) {
            EXCALIBUR_LOGE(TAG, "NVS flash init failed");
            return -1;
        }

        err = nvs_open("excalibur_ota", NVS_READWRITE, &nvs_handle);
        if (err != ESP_OK) {
            EXCALIBUR_LOGE(TAG, "failed to open OTA NVS storage");
            return -1;
        }

        nvs_set_i32(nvs_handle, "update_flag", update_flag);
        nvs_set_str(nvs_handle, "action_id", action_id);
        err = nvs_commit(nvs_handle);
        nvs_close(nvs_handle);
        if (err != ESP_OK) {
            EXCALIBUR_LOGE(TAG, "failed to commit OTA NVS data");
            return -1;
        }

        excalibur_hal_restart();
    } else {
        EXCALIBUR_LOGE(TAG, "firmware upgrade failed");
        excalibur_publish_action_status(excalibur_client, action_id, 0, EXCALIBUR_COMMAND_FAILED, excalibur_ota_error_str);
        memset(excalibur_ota_error_str, 0x00, sizeof(excalibur_ota_error_str));
        return -1;
    }

    return 0;
}

excalibur_err_t excalibur_handle_ota(excalibur_client_t *excalibur_client, char *payload_string, char *action_id)
{
    if (excalibur_client == NULL || action_id == NULL || payload_string == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    char constructed_url[EXCALIBUR_OTA_URL_STR_LEN] = {0};
    if (parse_ota_json(payload_string, constructed_url, sizeof(constructed_url)) == -1) {
        excalibur_publish_action_status(excalibur_client, action_id, 0, EXCALIBUR_COMMAND_FAILED, "Invalid OTA payload");
        return EXCALIBUR_FAILURE;
    }

    snprintf(excalibur_ota_action_id, EXCALIBUR_ACTION_ID_STR_LEN, "%s", action_id);
    if (perform_ota(excalibur_client, action_id, constructed_url) == -1) {
        return EXCALIBUR_FAILURE;
    }

    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_enable_ota(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    return excalibur_add_action_handler(excalibur_client, excalibur_handle_ota, "ota.install");
}
