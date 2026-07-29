#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "cJSON.h"
#include "excalibur_hal.h"
#include "excalibur_stream.h"
#include "excalibur_log.h"

static bool is_cloud_logging_enabled = false;
static char excalibur_log_stream[EXCALIBUR_LOG_STREAM_STR_LEN] = "";
static excalibur_log_level_t excalibur_log_level = EXCALIBUR_LOG_LEVEL_INFO;
static excalibur_client_t *excalibur_log_client = NULL;
static uint64_t log_sequence = 0;

static const char *TAG = "EXCALIBUR_LOG";

void excalibur_log_client_set(excalibur_client_t *excalibur_client)
{
    excalibur_log_client = excalibur_client;
}

void excalibur_enable_cloud_logging(void)
{
    is_cloud_logging_enabled = true;
}

bool excalibur_is_cloud_logging_enabled(void)
{
    return is_cloud_logging_enabled;
}

void excalibur_disable_cloud_logging(void)
{
    is_cloud_logging_enabled = false;
}

void excalibur_log_level_set(excalibur_log_level_t level)
{
    excalibur_log_level = level;
}

excalibur_log_level_t excalibur_log_level_get(void)
{
    return excalibur_log_level;
}

excalibur_err_t excalibur_log_stream_set(char *stream_name)
{
    if (stream_name == NULL) {
        return EXCALIBUR_NULL_CHECK_FAILURE;
    }
    int written = snprintf(excalibur_log_stream, sizeof(excalibur_log_stream), "%s", stream_name);
    return (written < 0 || written >= (int)sizeof(excalibur_log_stream)) ? EXCALIBUR_FAILURE : EXCALIBUR_SUCCESS;
}

char *excalibur_log_stream_get(void)
{
    return excalibur_log_stream;
}

excalibur_err_t excalibur_log_publish(const char *level, const char *tag, const char *fmt, ...)
{
    if (excalibur_log_client == NULL || !is_cloud_logging_enabled) {
        return EXCALIBUR_FAILURE;
    }

    va_list args;
    va_start(args, fmt);
    int buffer_size = vsnprintf(NULL, 0, fmt, args) + 1;
    va_end(args);

    if (buffer_size <= 0) {
        return EXCALIBUR_FAILURE;
    }

    char *message_buffer = malloc((size_t)buffer_size);
    if (message_buffer == NULL) {
        return EXCALIBUR_FAILURE;
    }

    va_start(args, fmt);
    vsnprintf(message_buffer, (size_t)buffer_size, fmt, args);
    va_end(args);

    cJSON *log_json = cJSON_CreateObject();
    if (log_json == NULL) {
        free(message_buffer);
        return EXCALIBUR_FAILURE;
    }
    cJSON_AddStringToObject(log_json, "level", level);
    cJSON_AddStringToObject(log_json, "tag", tag);
    cJSON_AddStringToObject(log_json, "message", message_buffer);

    char *log_string_json = cJSON_PrintUnformatted(log_json);
    cJSON_Delete(log_json);
    free(message_buffer);
    if (log_string_json == NULL) {
        return EXCALIBUR_FAILURE;
    }

    excalibur_err_t ret = excalibur_publish_telemetry(excalibur_log_client, excalibur_log_stream, log_sequence++, log_string_json);
    cJSON_free(log_string_json);
    if (ret != EXCALIBUR_SUCCESS) {
        EXCALIBUR_LOGE(TAG, "failed to publish cloud log");
    }
    return ret;
}
