#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "excalibur_sdk.h"

static excalibur_client_t excalibur_client = {0};

void app_main(void)
{
    if (excalibur_init(&excalibur_client) != EXCALIBUR_SUCCESS) {
        return;
    }
    excalibur_enable_ota(&excalibur_client);
    excalibur_start(&excalibur_client);

    while (1) {
        vTaskDelay(1000 / portTICK_PERIOD_MS);
    }
}
