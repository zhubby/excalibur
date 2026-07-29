#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cJSON.h"
#include "esp_idf_version.h"
#include "excalibur_hal.h"
#include "excalibur_log.h"
#include "excalibur_action.h"
#include "excalibur_ota.h"
#include "excalibur_stream.h"
#include "excalibur_client.h"

static cJSON *excalibur_cert_json = NULL;
static char *excalibur_device_config_data = NULL;
static char *excalibur_private_key_data = NULL;

static const char *TAG = "EXCALIBUR_CLIENT";

static char *read_file_alloc(const char *path)
{
    FILE *file = fopen(path, "r");
    if (file == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to open %s for reading", path);
        return NULL;
    }

    fseek(file, 0, SEEK_END);
    long file_length = ftell(file);
    fseek(file, 0, SEEK_SET);

    if (file_length <= 0) {
        EXCALIBUR_LOGE(TAG, "failed to get file size for %s", path);
        fclose(file);
        return NULL;
    }

    char *buffer = malloc((size_t)file_length + 1);
    if (buffer == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to allocate file buffer");
        fclose(file);
        return NULL;
    }

    size_t read_len = fread(buffer, 1, (size_t)file_length, file);
    buffer[read_len] = '\0';
    fclose(file);
    return buffer;
}

static int mount_provisioning_fs(char *config_fname, size_t config_fname_len)
{
    int ret_code = 0;
    memset(config_fname, 0x00, config_fname_len);

#if CONFIG_EXCALIBUR_PROVISIONING_FILESYSTEM_IS_SPIFFS
    EXCALIBUR_LOGI(TAG, "SPIFFS file system detected");
    ret_code = excalibur_hal_spiffs_mount();
    if (ret_code != 0) {
        return -1;
    }
    snprintf(config_fname, config_fname_len, "/spiffs/%s", CONFIG_EXCALIBUR_PROVISIONING_FILENAME);
#endif

#if CONFIG_EXCALIBUR_PROVISIONING_FILESYSTEM_IS_FATFS
    EXCALIBUR_LOGI(TAG, "FATFS file system detected");
    ret_code = excalibur_hal_fatfs_mount();
    if (ret_code != 0) {
        return -1;
    }
    snprintf(config_fname, config_fname_len, "/spiflash/%s", CONFIG_EXCALIBUR_PROVISIONING_FILENAME);
#endif

#if CONFIG_EXCALIBUR_PROVISIONING_FILESYSTEM_IS_LITTLEFS
    EXCALIBUR_LOGI(TAG, "LITTLEFS is not supported by this SDK");
    return -1;
#endif

    if (config_fname[0] == '\0') {
        EXCALIBUR_LOGE(TAG, "no provisioning file system selected");
        return -1;
    }

    return 0;
}

static int unmount_provisioning_fs(void)
{
#if CONFIG_EXCALIBUR_PROVISIONING_FILESYSTEM_IS_SPIFFS
    return excalibur_hal_spiffs_unmount();
#endif
#if CONFIG_EXCALIBUR_PROVISIONING_FILESYSTEM_IS_FATFS
    return excalibur_hal_fatfs_unmount();
#endif
    return 0;
}

static int copy_json_string(cJSON *root, const char *name, char *target, size_t target_len)
{
    cJSON *item = cJSON_GetObjectItem(root, name);
    if (!(cJSON_IsString(item) && item->valuestring != NULL)) {
        EXCALIBUR_LOGE(TAG, "missing or invalid %s", name);
        return -1;
    }

    int written = snprintf(target, target_len, "%s", item->valuestring);
    if (written < 0 || written >= (int)target_len) {
        EXCALIBUR_LOGE(TAG, "%s exceeded buffer size", name);
        return -1;
    }

    return 0;
}

static int parse_device_config_data(excalibur_device_config_t *device_cfg)
{
    if (excalibur_device_config_data == NULL) {
        EXCALIBUR_LOGE(TAG, "device config file is empty");
        return -1;
    }

    excalibur_cert_json = cJSON_Parse(excalibur_device_config_data);
    if (excalibur_cert_json == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to parse device config JSON");
        return -1;
    }

    if (copy_json_string(excalibur_cert_json, "project_id", device_cfg->project_id, sizeof(device_cfg->project_id)) != 0) {
        return -1;
    }
    if (copy_json_string(excalibur_cert_json, "device_id", device_cfg->device_id, sizeof(device_cfg->device_id)) != 0) {
        return -1;
    }

    cJSON *broker_name_obj = cJSON_GetObjectItem(excalibur_cert_json, "broker");
    cJSON *port_num_obj = cJSON_GetObjectItem(excalibur_cert_json, "port");
    if (!(cJSON_IsString(broker_name_obj) && broker_name_obj->valuestring != NULL) || !cJSON_IsNumber(port_num_obj)) {
        EXCALIBUR_LOGE(TAG, "missing broker or port");
        return -1;
    }

    int port_int = (int)port_num_obj->valuedouble;
    int written = snprintf(device_cfg->broker_uri, sizeof(device_cfg->broker_uri), "mqtts://%s:%d", broker_name_obj->valuestring, port_int);
    if (written < 0 || written >= (int)sizeof(device_cfg->broker_uri)) {
        EXCALIBUR_LOGE(TAG, "broker URI exceeded buffer size");
        return -1;
    }

    cJSON *auth_obj = cJSON_GetObjectItem(excalibur_cert_json, "authentication");
    if (!(cJSON_IsObject(auth_obj))) {
        EXCALIBUR_LOGE(TAG, "missing authentication object");
        return -1;
    }

    cJSON *ca_cert_obj = cJSON_GetObjectItem(auth_obj, "ca_certificate");
    cJSON *device_cert_obj = cJSON_GetObjectItem(auth_obj, "device_certificate");
    if (!(cJSON_IsString(ca_cert_obj) && ca_cert_obj->valuestring != NULL) ||
        !(cJSON_IsString(device_cert_obj) && device_cert_obj->valuestring != NULL)) {
        EXCALIBUR_LOGE(TAG, "missing device certificates");
        return -1;
    }

    device_cfg->ca_cert_pem = ca_cert_obj->valuestring;
    device_cfg->client_cert_pem = device_cert_obj->valuestring;

    cJSON *device_private_key_obj = cJSON_GetObjectItem(auth_obj, "device_private_key");
    if (cJSON_IsString(device_private_key_obj) && device_private_key_obj->valuestring != NULL) {
        device_cfg->client_key_pem = device_private_key_obj->valuestring;
        return 0;
    }

    cJSON *device_private_key_path_obj = cJSON_GetObjectItem(auth_obj, "device_private_key_path");
    if (!(cJSON_IsString(device_private_key_path_obj) && device_private_key_path_obj->valuestring != NULL)) {
        EXCALIBUR_LOGE(TAG, "missing device_private_key or device_private_key_path");
        return -1;
    }

    excalibur_private_key_data = read_file_alloc(device_private_key_path_obj->valuestring);
    if (excalibur_private_key_data == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to read private key from path");
        return -1;
    }

    device_cfg->client_key_pem = excalibur_private_key_data;
    return 0;
}

static int read_device_config_from_filesystem(excalibur_device_config_t *device_cfg)
{
    char config_fname[128] = "";

    if (mount_provisioning_fs(config_fname, sizeof(config_fname)) != 0) {
        return -1;
    }

    EXCALIBUR_LOGI(TAG, "reading file: %s", config_fname);
    excalibur_device_config_data = read_file_alloc(config_fname);
    if (excalibur_device_config_data == NULL) {
        unmount_provisioning_fs();
        return -1;
    }

    int ret = parse_device_config_data(device_cfg);
    free(excalibur_device_config_data);
    excalibur_device_config_data = NULL;

    if (unmount_provisioning_fs() != 0) {
        EXCALIBUR_LOGE(TAG, "failed to unmount provisioning file system");
        return -1;
    }

    return ret;
}

static void set_mqtt_conf(excalibur_device_config_t *device_cfg, excalibur_client_config_t *mqtt_cfg)
{
#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 0, 0)
    mqtt_cfg->broker.address.uri = device_cfg->broker_uri;
    mqtt_cfg->broker.verification.certificate = (const char *)device_cfg->ca_cert_pem;
    mqtt_cfg->credentials.authentication.certificate = (const char *)device_cfg->client_cert_pem;
    mqtt_cfg->credentials.authentication.key = (const char *)device_cfg->client_key_pem;
#else
    mqtt_cfg->uri = device_cfg->broker_uri;
    mqtt_cfg->cert_pem = (const char *)device_cfg->ca_cert_pem;
    mqtt_cfg->client_cert_pem = (const char *)device_cfg->client_cert_pem;
    mqtt_cfg->client_key_pem = (const char *)device_cfg->client_key_pem;
#endif
}

static void excalibur_sdk_cleanup(excalibur_client_t *excalibur_client)
{
    excalibur_client->device_cfg.ca_cert_pem = NULL;
    excalibur_client->device_cfg.client_cert_pem = NULL;
    excalibur_client->device_cfg.client_key_pem = NULL;
    memset(excalibur_client->device_cfg.broker_uri, 0x00, sizeof(excalibur_client->device_cfg.broker_uri));
    memset(excalibur_client->device_cfg.device_id, 0x00, sizeof(excalibur_client->device_cfg.device_id));
    memset(excalibur_client->device_cfg.project_id, 0x00, sizeof(excalibur_client->device_cfg.project_id));
    excalibur_client->client = NULL;
    memset(&(excalibur_client->mqtt_cfg), 0x00, sizeof(excalibur_client->mqtt_cfg));
    excalibur_reset_action_handler_array(excalibur_client);
    excalibur_client->connection_status = 0;
    excalibur_ota_action_id[0] = '\0';
    excalibur_log_client_set(NULL);
    excalibur_log_level_set(EXCALIBUR_LOG_LEVEL_INFO);

    if (excalibur_cert_json != NULL) {
        cJSON_Delete(excalibur_cert_json);
        excalibur_cert_json = NULL;
    }
    if (excalibur_private_key_data != NULL) {
        free(excalibur_private_key_data);
        excalibur_private_key_data = NULL;
    }
}

const char *excalibur_command_state_to_string(excalibur_command_state_t state)
{
    switch (state) {
    case EXCALIBUR_COMMAND_COMPLETED:
        return "Completed";
    case EXCALIBUR_COMMAND_FAILED:
        return "Failed";
    case EXCALIBUR_COMMAND_CANCELLED:
        return "Cancelled";
    case EXCALIBUR_COMMAND_TIMED_OUT:
        return "TimedOut";
    case EXCALIBUR_COMMAND_RUNNING:
    default:
        return "Running";
    }
}

int excalibur_build_telemetry_topic(excalibur_device_config_t device_cfg, const char *stream_name, char *topic, size_t topic_len)
{
    int written = snprintf(topic, topic_len, "v1/p/%s/d/%s/telemetry/%s", device_cfg.project_id, device_cfg.device_id, stream_name);
    return (written < 0 || written >= (int)topic_len) ? -1 : 0;
}

int excalibur_build_shadow_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len)
{
    int written = snprintf(topic, topic_len, "v1/p/%s/d/%s/shadow", device_cfg.project_id, device_cfg.device_id);
    return (written < 0 || written >= (int)topic_len) ? -1 : 0;
}

int excalibur_build_commands_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len)
{
    int written = snprintf(topic, topic_len, "v1/p/%s/d/%s/commands", device_cfg.project_id, device_cfg.device_id);
    return (written < 0 || written >= (int)topic_len) ? -1 : 0;
}

int excalibur_build_command_status_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len)
{
    int written = snprintf(topic, topic_len, "v1/p/%s/d/%s/commands/status", device_cfg.project_id, device_cfg.device_id);
    return (written < 0 || written >= (int)topic_len) ? -1 : 0;
}

excalibur_err_t excalibur_init(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    int ret_val = 0;
    if (excalibur_client->use_device_config_data == false) {
        ret_val = read_device_config_from_filesystem(&(excalibur_client->device_cfg));
        if (ret_val != 0) {
            EXCALIBUR_LOGE(TAG, "error reading device config JSON");
            excalibur_sdk_cleanup(excalibur_client);
            return EXCALIBUR_FAILURE;
        }
    } else {
        EXCALIBUR_LOGI(TAG, "using provided device config data");
    }

    set_mqtt_conf(&(excalibur_client->device_cfg), &(excalibur_client->mqtt_cfg));

    ret_val = excalibur_hal_init(excalibur_client);
    if (ret_val != 0) {
        EXCALIBUR_LOGE(TAG, "error initializing Excalibur HAL");
        excalibur_sdk_cleanup(excalibur_client);
        return EXCALIBUR_FAILURE;
    }

    excalibur_log_client_set(excalibur_client);
    excalibur_log_level_set(CONFIG_EXCALIBUR_LOGGING_LEVEL);

#if CONFIG_EXCALIBUR_CLOUD_LOGGING_IS_ENABLED
    excalibur_enable_cloud_logging();
    excalibur_log_stream_set(CONFIG_EXCALIBUR_CLOUD_LOGGING_STREAM);
#endif

    EXCALIBUR_LOGI(TAG, "Excalibur client initialized");
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_start(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    return excalibur_hal_start_mqtt(excalibur_client) == 0 ? EXCALIBUR_SUCCESS : EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_stop(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    return excalibur_hal_stop_mqtt(excalibur_client) == 0 ? EXCALIBUR_SUCCESS : EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_destroy(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    if (excalibur_hal_destroy(excalibur_client) != 0) {
        return EXCALIBUR_FAILURE;
    }
    excalibur_sdk_cleanup(excalibur_client);
    return EXCALIBUR_SUCCESS;
}
