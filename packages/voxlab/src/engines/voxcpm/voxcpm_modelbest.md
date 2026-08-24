# Bash API documentation for https://voxcpm.modelbest.cn/

API Endpoints: 4

1. Confirm that you have cURL installed on your system.

```bash
curl --version
```

2. Find the API endpoint below corresponding to your desired function in the app. Copy the code snippet, replacing the placeholder values with your own input data.

Making a prediction and getting a result requires 2 requests: a POST and a GET request. The POST request returns an EVENT_ID, which is used in the second GET request to fetch the results. In these snippets, we've used awk and read to parse the results, combining these two requests into one command for ease of use. See [curl docs](https://www.gradio.app/guides/querying-gradio-apps-with-curl).

### API Name: /_on_toggle_instant

```bash
curl -X POST http://localhost:7865/gradio_api/call/_on_toggle_instant -s -H "Content-Type: application/json" -d '{"data": [false]}' \
  | awk -F'"' '{ print $4}' \
  | read EVENT_ID; curl -N http://localhost:7865/gradio_api/call/_on_toggle_instant/$EVENT_ID
```

Accepts 1 parameter:

[0]:

- Type: boolean
- Required
- The input value that is provided in the [object Object] Checkbox component.

Returns list of 2 elements:

[0]: - Type: string

- The output value that appears in the "[object Object]" Textbox component.

[1]: - Type: string

- The output value that appears in the "[object Object]" Textbox component.

### API Name: /_run_asr_if_needed

```bash
curl -X POST http://localhost:7865/gradio_api/call/_run_asr_if_needed -s -H "Content-Type: application/json" -d '{"data": [false, {"path": "https://github.com/gradio-app/gradio/raw/main/test/test_files/audio_sample.wav", "meta": {"_type": "gradio.FileData"}}]}' \
  | awk -F'"' '{ print $4}' \
  | read EVENT_ID; curl -N http://localhost:7865/gradio_api/call/_run_asr_if_needed/$EVENT_ID
```

Accepts 2 parameters:

[0]:

- Type: boolean
- Required
- The input value that is provided in the [object Object] Checkbox component.

[1]:

- Type: any
- Required
- The input value that is provided in the [object Object] Audio component. The FileData class is a subclass of the GradioModel class that represents a file object within a Gradio interface. It is used to store file data and metadata when a file is uploaded.

Attributes:
path: The server file path where the file is stored.
url: The normalized server URL pointing to the file.
size: The size of the file in bytes.
orig_name: The original filename before upload.
mime_type: The MIME type of the file.
is_stream: Indicates whether the file is a stream.
meta: Additional metadata used internally (should not be changed).

Returns 1 element:

- Type: string
- The output value that appears in the "[object Object]" Textbox component.

### API Name: /generate

```bash
curl -X POST http://localhost:7865/gradio_api/call/generate -s -H "Content-Type: application/json" -d '{"data": ["VoxCPM2 is a creative multilingual TTS model from ModelBest, designed to generate highly realistic speech.", "", {"path": "https://github.com/gradio-app/gradio/raw/main/test/test_files/audio_sample.wav", "meta": {"_type": "gradio.FileData"}}, false, "", 2.0, false, false, 10, "Hello!!"]}' \
  | awk -F'"' '{ print $4}' \
  | read EVENT_ID; curl -N http://localhost:7865/gradio_api/call/generate/$EVENT_ID
```

Accepts 10 parameters:

[0]:

- Type: string
- Required
- The input value that is provided in the [object Object] Textbox component.

[1]:

- Type: string
- Required
- The input value that is provided in the [object Object] Textbox component.

[2]:

- Type: any
- Required
- The input value that is provided in the [object Object] Audio component. The FileData class is a subclass of the GradioModel class that represents a file object within a Gradio interface. It is used to store file data and metadata when a file is uploaded.

Attributes:
path: The server file path where the file is stored.
url: The normalized server URL pointing to the file.
size: The size of the file in bytes.
orig_name: The original filename before upload.
mime_type: The MIME type of the file.
is_stream: Indicates whether the file is a stream.
meta: Additional metadata used internally (should not be changed).

[3]:

- Type: boolean
- Required
- The input value that is provided in the [object Object] Checkbox component.

[4]:

- Type: string
- Required
- The input value that is provided in the [object Object] Textbox component.

[5]:

- Type: number
- Required
- The input value that is provided in the [object Object] Slider component.

[6]:

- Type: boolean
- Required
- The input value that is provided in the [object Object] Checkbox component.

[7]:

- Type: boolean
- Required
- The input value that is provided in the [object Object] Checkbox component.

[8]:

- Type: number
- Required
- The input value that is provided in the [object Object] Slider component.

[9]:

- Type: string
- Required
- The input value that is provided in the parameter_3 Textbox component.

Returns 1 element:

- Type:
- The output value that appears in the "[object Object]" Audio component.

### API Name: /_save_downloaded_audio

```bash
curl -X POST http://localhost:7865/gradio_api/call/_save_downloaded_audio -s -H "Content-Type: application/json" -d '{"data": ["Hello!!", "Hello!!"]}' \
  | awk -F'"' '{ print $4}' \
  | read EVENT_ID; curl -N http://localhost:7865/gradio_api/call/_save_downloaded_audio/$EVENT_ID
```

Accepts 2 parameters:

[0]:

- Type: string
- Required
- The input value that is provided in the parameter_2 Textbox component.

[2]:

- Type: string
- Required
- The input value that is provided in the parameter_3 Textbox component.

Returns 1 element:
