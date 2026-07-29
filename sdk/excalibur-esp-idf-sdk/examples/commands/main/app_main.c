#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "excalibur_sdk.h"

static excalibur_client_t excalibur_client = {0};

static int handle_toggle(excalibur_client_t *client, char *payload_json, char *action_id)
{
    (void)payload_json;
    return excalibur_publish_action_completed(client, action_id);
}

void app_main(void)
{
    if (excalibur_init(&excalibur_client) != EXCALIBUR_SUCCESS) {
        return;
    }
    excalibur_add_action_handler(&excalibur_client, handle_toggle, "toggle");
    excalibur_start(&excalibur_client);

    while (1) {
        vTaskDelay(1000 / portTICK_PERIOD_MS);
    }
}
