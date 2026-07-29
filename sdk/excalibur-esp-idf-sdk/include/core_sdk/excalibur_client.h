#ifndef EXCALIBUR_CLIENT_H
#define EXCALIBUR_CLIENT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include "mqtt_client.h"
#include "sdkconfig.h"

#define EXCALIBUR_BROKER_URL_STR_LEN 128
#define EXCALIBUR_UUID_STR_LEN 37
#define EXCALIBUR_PROJECT_ID_STR_LEN EXCALIBUR_UUID_STR_LEN
#define EXCALIBUR_DEVICE_ID_STR_LEN EXCALIBUR_UUID_STR_LEN
#define EXCALIBUR_ACTION_ID_STR_LEN EXCALIBUR_UUID_STR_LEN
#define EXCALIBUR_MQTT_TOPIC_STR_LEN 256
#define EXCALIBUR_OTA_URL_STR_LEN 512
#define EXCALIBUR_NUMBER_OF_ACTIONS 10

struct excalibur_client;
typedef esp_mqtt_client_handle_t excalibur_client_handle_t;
typedef esp_mqtt_client_config_t excalibur_client_config_t;

typedef enum excalibur_err {
    EXCALIBUR_SUCCESS = 0,
    EXCALIBUR_FAILURE = -1,
    EXCALIBUR_NULL_CHECK_FAILURE = -2,
    EXCALIBUR_PROGRESS_OUT_OF_RANGE = -3
} excalibur_err_t;

typedef enum excalibur_command_state {
    EXCALIBUR_COMMAND_RUNNING,
    EXCALIBUR_COMMAND_COMPLETED,
    EXCALIBUR_COMMAND_FAILED,
    EXCALIBUR_COMMAND_CANCELLED,
    EXCALIBUR_COMMAND_TIMED_OUT
} excalibur_command_state_t;

typedef struct excalibur_device_config {
    char *ca_cert_pem;
    char *client_cert_pem;
    char *client_key_pem;
    char broker_uri[EXCALIBUR_BROKER_URL_STR_LEN];
    char device_id[EXCALIBUR_DEVICE_ID_STR_LEN];
    char project_id[EXCALIBUR_PROJECT_ID_STR_LEN];
} excalibur_device_config_t;

typedef struct excalibur_action_functions_map {
    const char *name;
    int (*func)(struct excalibur_client *excalibur_client, char *payload_json, char *action_id);
} excalibur_action_functions_map_t;

typedef struct excalibur_device_shadow {
    char custom_json_str[CONFIG_EXCALIBUR_SHADOW_CUSTOM_JSON_STR_LEN];
    int (*updater)(struct excalibur_client *excalibur_client);
    uint64_t sequence;
} excalibur_device_shadow_t;

typedef struct excalibur_client {
    excalibur_device_config_t device_cfg;
    excalibur_client_handle_t client;
    excalibur_client_config_t mqtt_cfg;
    excalibur_action_functions_map_t action_funcs[EXCALIBUR_NUMBER_OF_ACTIONS];
    excalibur_device_shadow_t device_shadow;
    int connection_status;
    bool use_device_config_data;
} excalibur_client_t;

excalibur_err_t excalibur_init(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_start(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_stop(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_destroy(excalibur_client_t *excalibur_client);

const char *excalibur_command_state_to_string(excalibur_command_state_t state);
int excalibur_build_telemetry_topic(excalibur_device_config_t device_cfg, const char *stream_name, char *topic, size_t topic_len);
int excalibur_build_shadow_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len);
int excalibur_build_commands_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len);
int excalibur_build_command_status_topic(excalibur_device_config_t device_cfg, char *topic, size_t topic_len);

#endif
