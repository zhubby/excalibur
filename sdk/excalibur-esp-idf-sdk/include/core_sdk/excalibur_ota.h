#ifndef EXCALIBUR_OTA_H
#define EXCALIBUR_OTA_H

#include "excalibur_client.h"

#define EXCALIBUR_OTA_ERROR_STR_LEN 200

extern char excalibur_ota_action_id[EXCALIBUR_ACTION_ID_STR_LEN];
extern char excalibur_ota_error_str[EXCALIBUR_OTA_ERROR_STR_LEN];

excalibur_err_t excalibur_handle_ota(excalibur_client_t *excalibur_client, char *payload_string, char *action_id);
excalibur_err_t excalibur_enable_ota(excalibur_client_t *excalibur_client);

#endif
