#ifndef EXCALIBUR_STREAM_H
#define EXCALIBUR_STREAM_H

#include "excalibur_client.h"

excalibur_err_t excalibur_publish_telemetry(excalibur_client_t *excalibur_client, char *stream_name, uint64_t sequence, char *payload_object_json);
excalibur_err_t excalibur_publish_shadow(excalibur_client_t *excalibur_client, char *shadow_object_json);
excalibur_err_t excalibur_publish_device_shadow(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_add_custom_device_shadow(excalibur_client_t *excalibur_client, char *custom_json_str);
excalibur_err_t excalibur_register_device_shadow_updater(excalibur_client_t *excalibur_client, int (*func_ptr)(excalibur_client_t *));
excalibur_err_t excalibur_batch_init(excalibur_client_t *excalibur_client, char *stream_name);
excalibur_err_t excalibur_batch_publish_telemetry(char *payload_object_json);
void excalibur_user_thread_entry(void *pv);
void excalibur_mqtt_thread_entry(void *pv);

#endif
