mfl-gramophone
==============

![AGPLv3 licensed](https://www.gnu.org/graphics/agplv3-155x51.png)

A simple Rust application to play audio, using JACK, when it receives commands via OSC.

## Logging

**Note**: `export RUST_LOG=mfl_gramophone=INFO` if you want any useful logging information.

## Configuration

See `mfl-gramophone.toml.example`, and rename it to `mfl-gramophone.toml`.

## OSC Dictionary

- **Note**: OSC bundles are unsupported and will be ignored.

### Replies

- All commands will generate an `/ack` reply (*including* `/ping`), regardless
  of whether or not they are successful.
  - (This is arguably a bug; read the logs if you care about error output.)
- All replies are sent to the same address that the OSC packet was received
  from.

### `/ping`

- Does nothing, apart from sending an `/ack` reply like every other command.

### `/shutdown`

- Instantly shuts the server down, halting all audio playback.

### `/file/{name}`

- **Note**: These commands begin with `/file/`, **NOT** `/files/`!

#### `/start LEVEL`

- Starts playing the file `{name}`, as specified in the config file.
- `LEVEL` (type float or double): volume, in decibels, to begin playback at.

#### `/stop`

- Stops playing the file `{name}`, as specified in the config file.

#### `/fade LEVEL DURATION`

- Gradually changes the volume of a file over time.
- `LEVEL` (type float or double): volume, in decibels, to end up at.
- `DURATION` (type integer): duration, in **milliseconds**, to fade over.

#### `/debug`

- Spews a bunch of debug information about the file `{name}` to the logs.

## Other caveats

- Resampling is not supported; everything has to be the same sample rate (files,
  JACK)

