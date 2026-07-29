#ifndef EXCALIBUR_ACTION_H
#define EXCALIBUR_ACTION_H

#include "excalibur_client.h"

excalibur_err_t excalibur_add_action_handler(excalibur_client_t *excalibur_client, int (*func_ptr)(excalibur_client_t *, char *, char *), char *func_name);
excalibur_err_t excalibur_remove_action_handler(excalibur_client_t *excalibur_client, char *func_name);
excalibur_err_t excalibur_update_action_handler(excalibur_client_t *excalibur_client, int (*new_func_ptr)(excalibur_client_t *, char *, char *), char *func_name);
excalibur_err_t excalibur_is_action_handler_there(excalibur_client_t *excalibur_client, char *func_name);
excalibur_err_t excalibur_print_action_handler_array(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_reset_action_handler_array(excalibur_client_t *excalibur_client);
excalibur_err_t excalibur_publish_action_completed(excalibur_client_t *excalibur_client, char *action_id);
excalibur_err_t excalibur_publish_action_failed(excalibur_client_t *excalibur_client, char *action_id);
excalibur_err_t excalibur_publish_action_running(excalibur_client_t *excalibur_client, char *action_id, int progress_percentage);
excalibur_err_t excalibur_publish_action_status(excalibur_client_t *excalibur_client, char *action_id, int percentage, excalibur_command_state_t state, char *error_message);

int excalibur_subscribe_to_commands(excalibur_device_config_t device_cfg, excalibur_client_handle_t client);
int excalibur_unsubscribe_to_commands(excalibur_device_config_t device_cfg, excalibur_client_handle_t client);
int excalibur_handle_command(char *command_received, excalibur_client_t *excalibur_client);

#endif
