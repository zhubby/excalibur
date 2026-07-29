#ifndef EXCALIBUR_LOG_H
#define EXCALIBUR_LOG_H

#include <stdarg.h>
#include <stdbool.h>
#include "excalibur_client.h"

#define EXCALIBUR_LOG_STREAM_STR_LEN 64

typedef enum excalibur_log_level {
    EXCALIBUR_LOG_LEVEL_NONE,
    EXCALIBUR_LOG_LEVEL_ERROR,
    EXCALIBUR_LOG_LEVEL_WARN,
    EXCALIBUR_LOG_LEVEL_INFO,
    EXCALIBUR_LOG_LEVEL_DEBUG,
    EXCALIBUR_LOG_LEVEL_VERBOSE
} excalibur_log_level_t;

void excalibur_log_client_set(excalibur_client_t *excalibur_client);
void excalibur_enable_cloud_logging(void);
bool excalibur_is_cloud_logging_enabled(void);
void excalibur_disable_cloud_logging(void);
void excalibur_log_level_set(excalibur_log_level_t level);
excalibur_log_level_t excalibur_log_level_get(void);
excalibur_err_t excalibur_log_stream_set(char *stream_name);
char *excalibur_log_stream_get(void);
excalibur_err_t excalibur_log_publish(const char *level, const char *tag, const char *fmt, ...);

#endif
