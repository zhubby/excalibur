#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cJSON.h"
#include "excalibur_hal.h"
#include "excalibur_action.h"

static int function_handler_index = 0;
static const char *TAG = "EXCALIBUR_ACTION";

int excalibur_subscribe_to_commands(excalibur_device_config_t device_cfg, excalibur_client_handle_t client)
{
    int qos = 1;
    char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};

    if (excalibur_build_commands_topic(device_cfg, topic, sizeof(topic)) != 0) {
        EXCALIBUR_LOGE(TAG, "subscribe topic size exceeded buffer size");
        return -1;
    }

    return excalibur_hal_mqtt_subscribe(client, topic, qos);
}

int excalibur_unsubscribe_to_commands(excalibur_device_config_t device_cfg, excalibur_client_handle_t client)
{
    char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};

    if (excalibur_build_commands_topic(device_cfg, topic, sizeof(topic)) != 0) {
        EXCALIBUR_LOGE(TAG, "unsubscribe topic size exceeded buffer size");
        return -1;
    }

    return excalibur_hal_mqtt_unsubscribe(client, topic);
}

static char *duplicate_string(const char *value)
{
    size_t len = strlen(value);
    char *copy = malloc(len + 1);
    if (copy == NULL) {
        return NULL;
    }
    memcpy(copy, value, len + 1);
    return copy;
}

static char *payload_to_handler_string(cJSON *payload)
{
    if (payload == NULL) {
        return duplicate_string("{}");
    }
    if (cJSON_IsString(payload) && payload->valuestring != NULL) {
        return duplicate_string(payload->valuestring);
    }
    return cJSON_PrintUnformatted(payload);
}

int excalibur_handle_command(char *command_received, excalibur_client_t *excalibur_client)
{
    if (command_received == NULL || excalibur_client == NULL) {
        return -1;
    }

    cJSON *root = cJSON_Parse(command_received);
    if (root == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to parse command JSON");
        return -1;
    }

    cJSON *name = cJSON_GetObjectItem(root, "name");
    if (!(cJSON_IsString(name) && name->valuestring != NULL)) {
        EXCALIBUR_LOGE(TAG, "missing command name");
        cJSON_Delete(root);
        return -1;
    }

    cJSON *action_id_obj = cJSON_GetObjectItem(root, "action_id");
    if (!(cJSON_IsString(action_id_obj) && action_id_obj->valuestring != NULL)) {
        EXCALIBUR_LOGE(TAG, "missing command action_id");
        cJSON_Delete(root);
        return -1;
    }

    char action_id[EXCALIBUR_ACTION_ID_STR_LEN] = {0};
    int written = snprintf(action_id, sizeof(action_id), "%s", action_id_obj->valuestring);
    if (written < 0 || written >= (int)sizeof(action_id)) {
        EXCALIBUR_LOGE(TAG, "action_id exceeded buffer size");
        cJSON_Delete(root);
        return -1;
    }

    cJSON *payload = cJSON_GetObjectItem(root, "payload");
    char *payload_string = payload_to_handler_string(payload);
    if (payload_string == NULL) {
        EXCALIBUR_LOGE(TAG, "failed to serialize command payload");
        cJSON_Delete(root);
        return -1;
    }

    for (int action_iterator = 0; action_iterator < EXCALIBUR_NUMBER_OF_ACTIONS; action_iterator++) {
        if (excalibur_client->action_funcs[action_iterator].name == NULL) {
            continue;
        }
        if (strcmp(excalibur_client->action_funcs[action_iterator].name, name->valuestring) == 0) {
            excalibur_client->action_funcs[action_iterator].func(excalibur_client, payload_string, action_id);
            free(payload_string);
            cJSON_Delete(root);
            return 0;
        }
    }

    EXCALIBUR_LOGI(TAG, "unregistered command: %s", name->valuestring);
    excalibur_publish_action_status(excalibur_client, action_id, 0, EXCALIBUR_COMMAND_FAILED, "Unregistered command");
    free(payload_string);
    cJSON_Delete(root);
    return 0;
}

excalibur_err_t excalibur_add_action_handler(excalibur_client_t *excalibur_client, int (*func_ptr)(excalibur_client_t *, char *, char *), char *func_name)
{
    if (excalibur_client == NULL || func_ptr == NULL || func_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    if (function_handler_index >= EXCALIBUR_NUMBER_OF_ACTIONS) {
        return EXCALIBUR_FAILURE;
    }

    for (int i = 0; i < function_handler_index; i++) {
        if (strcmp(excalibur_client->action_funcs[i].name, func_name) == 0) {
            return EXCALIBUR_FAILURE;
        }
    }

    excalibur_client->action_funcs[function_handler_index].func = func_ptr;
    excalibur_client->action_funcs[function_handler_index].name = func_name;
    function_handler_index++;
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_remove_action_handler(excalibur_client_t *excalibur_client, char *func_name)
{
    if (excalibur_client == NULL || func_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    int target_action_index = -1;
    for (int i = 0; i < function_handler_index; i++) {
        if (strcmp(excalibur_client->action_funcs[i].name, func_name) == 0) {
            target_action_index = i;
            break;
        }
    }

    if (target_action_index == -1) {
        return EXCALIBUR_FAILURE;
    }

    for (int i = target_action_index; i < function_handler_index - 1; i++) {
        excalibur_client->action_funcs[i] = excalibur_client->action_funcs[i + 1];
    }
    function_handler_index--;
    excalibur_client->action_funcs[function_handler_index].func = NULL;
    excalibur_client->action_funcs[function_handler_index].name = NULL;
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_update_action_handler(excalibur_client_t *excalibur_client, int (*new_func_ptr)(excalibur_client_t *, char *, char *), char *func_name)
{
    if (excalibur_client == NULL || new_func_ptr == NULL || func_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    for (int i = 0; i < function_handler_index; i++) {
        if (strcmp(excalibur_client->action_funcs[i].name, func_name) == 0) {
            excalibur_client->action_funcs[i].func = new_func_ptr;
            return EXCALIBUR_SUCCESS;
        }
    }
    return EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_is_action_handler_there(excalibur_client_t *excalibur_client, char *func_name)
{
    if (excalibur_client == NULL || func_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    for (int i = 0; i < function_handler_index; i++) {
        if (strcmp(excalibur_client->action_funcs[i].name, func_name) == 0) {
            return EXCALIBUR_SUCCESS;
        }
    }
    return EXCALIBUR_FAILURE;
}

excalibur_err_t excalibur_print_action_handler_array(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    for (int i = 0; i < EXCALIBUR_NUMBER_OF_ACTIONS; i++) {
        EXCALIBUR_LOGI(TAG, "handler[%d]=%s", i, excalibur_client->action_funcs[i].name ? excalibur_client->action_funcs[i].name : "NULL");
    }
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_reset_action_handler_array(excalibur_client_t *excalibur_client)
{
    if (excalibur_client == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }

    for (int i = 0; i < EXCALIBUR_NUMBER_OF_ACTIONS; i++) {
        excalibur_client->action_funcs[i].func = NULL;
        excalibur_client->action_funcs[i].name = NULL;
    }
    function_handler_index = 0;
    return EXCALIBUR_SUCCESS;
}

excalibur_err_t excalibur_publish_action_completed(excalibur_client_t *excalibur_client, char *action_id)
{
    return excalibur_publish_action_status(excalibur_client, action_id, 100, EXCALIBUR_COMMAND_COMPLETED, "");
}

excalibur_err_t excalibur_publish_action_failed(excalibur_client_t *excalibur_client, char *action_id)
{
    return excalibur_publish_action_status(excalibur_client, action_id, 0, EXCALIBUR_COMMAND_FAILED, "Action failed");
}

excalibur_err_t excalibur_publish_action_running(excalibur_client_t *excalibur_client, char *action_id, int progress_percentage)
{
    return excalibur_publish_action_status(excalibur_client, action_id, progress_percentage, EXCALIBUR_COMMAND_RUNNING, "");
}

excalibur_err_t excalibur_publish_action_status(excalibur_client_t *excalibur_client, char *action_id, int percentage, excalibur_command_state_t state, char *error_message)
{
    if (excalibur_client == NULL || action_id == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    if (percentage < 0) {
        percentage = 0;
    }
    if (percentage > 100) {
        percentage = 100;
    }

    cJSON *status_list = cJSON_CreateArray();
    cJSON *status_json = cJSON_CreateObject();
    if (status_list == NULL || status_json == NULL) {
        cJSON_Delete(status_list);
        cJSON_Delete(status_json);
        return EXCALIBUR_FAILURE;
    }

    cJSON_AddStringToObject(status_json, "action_id", action_id);
    cJSON_AddStringToObject(status_json, "state", excalibur_command_state_to_string(state));
    cJSON_AddNumberToObject(status_json, "progress", percentage);

    cJSON *errors = cJSON_CreateArray();
    if (errors == NULL) {
        cJSON_Delete(status_list);
        cJSON_Delete(status_json);
        return EXCALIBUR_FAILURE;
    }
    if (error_message != NULL && error_message[0] != '\0') {
        cJSON_AddItemToArray(errors, cJSON_CreateString(error_message));
    }
    cJSON_AddItemToObject(status_json, "errors", errors);
    cJSON_AddItemToArray(status_list, status_json);

    char *string_json = cJSON_PrintUnformatted(status_list);
    if (string_json == NULL) {
        cJSON_Delete(status_list);
        return EXCALIBUR_FAILURE;
    }

    char topic[EXCALIBUR_MQTT_TOPIC_STR_LEN] = {0};
    if (excalibur_build_command_status_topic(excalibur_client->device_cfg, topic, sizeof(topic)) != 0) {
        EXCALIBUR_LOGE(TAG, "command status topic exceeded buffer size");
        cJSON_free(string_json);
        cJSON_Delete(status_list);
        return EXCALIBUR_FAILURE;
    }

    int msg_id = excalibur_hal_mqtt_publish(excalibur_client->client, topic, string_json, strlen(string_json), 1);
    cJSON_free(string_json);
    cJSON_Delete(status_list);

    return msg_id != -1 ? EXCALIBUR_SUCCESS : EXCALIBUR_FAILURE;
}
