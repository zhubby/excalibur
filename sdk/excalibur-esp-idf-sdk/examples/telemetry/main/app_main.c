#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "excalibur_sdk.h"

static excalibur_client_t excalibur_client = {0};

void app_main(void)
{
    if (excalibur_init(&excalibur_client) != EXCALIBUR_SUCCESS) {
        return;
    }
    if (excalibur_start(&excalibur_client) != EXCALIBUR_SUCCESS) {
        return;
    }

    while (excalibur_client.connection_status != 1) {
        vTaskDelay(100 / portTICK_PERIOD_MS);
    }

    excalibur_publish_telemetry(
        &excalibur_client,
        "temperature",
        1,
        "{\"value\":24.6,\"status\":\"ok\"}"
    );
    excalibur_publish_shadow(
        &excalibur_client,
        "{\"health\":\"nominal\",\"firmware\":{\"app\":\"0.1.0\"}}"
    );
}
