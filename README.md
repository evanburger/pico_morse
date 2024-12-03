# pico_morse
## using Embassy

Blink the Pico W's onboard LED via HTTP messages using the Embassy framework.

## Examples
```bash
./morse_client '192.168.0.42:80' 'Hello Paris.'
# morse code blinking sequence: ....+.+._..+._..+___*.__.+._+._.+..+...
# where . means a 'dit', _ means a 'dah', + is a letter separator and * is a word separator.
```

## Installation
1. Set env vars according to target wireless LAN (the env vars must be exported to be accessible other processes):
* WIFI_NETWORK
* WIFI_PASSWORD
2. Connect Pico board via USB while in boot mode
3. Use `cargo run` from the project's root directory to flash image onto Pico board (this must be redone any time the parameters such wireless LAN SSID or password change)

## Usage
1. Optionally use `read_usb` while the Pico is connected to see which IP address it was assigned on the LAN (otherwise the Pico can just be connected to any power source if logging to USB is not required, such as when the IP hasn't changed or was statically configured)
2. Use the `morse_client` Python script with the IP and port such as '192.168.0.42:80' as the first argument and thne the text message like 'Hello Paris' as the second argument. The message should be sent to the board over local WiFi and will blink the LED in morse code
