mfl-gramophone
==============

![AGPLv3 licensed](https://www.gnu.org/graphics/agplv3-155x51.png)

A simple Rust application to play audio, using JACK, when it receives commands via OSC.

## Logging

**Note**: `export RUST_LOG=mfl_gramophone=INFO` if you want any useful logging informattion.

## Configuration

See `mfl-gramophone.toml.example`, and rename it to `mfl-gramophone.toml`.

## OSC Dictionary

- **Note**: OSC bundles are unsupported and will be ignored.

### Replies

- All commands will generate an `/ack` reply (apart from `/ping`), regardless
  of whether or not they are successful.
  - (This is arguably a bug; read the logs if you care about error output.)
- All replies are sent to the same address that the OSC packet was received
  from.

### `/ping`

- Replies with a `/pong` to the address it received the OSC packet from.

### `/shutdown`

- Shuts the server down, halting all audio playback, after waiting a bit.
  - **Note**: This is supposed to wait for all audio to stop playing but it
    doesn't because it's bugged. Caveat emptor.

### `/fast_shutdown`

- Instantly shuts the server down, halting all audio playback.

### `/files/{name}`

#### `/start`

- Starts playing the file `{name}`, as specified in the config file.

#### `/stop`

- Stops playing the file `{name}`, as specified in the config file.

#### `/fade_in TIME`

- Fades the file `{name}` in gradually, over a period of `TIME` **milliseconds**.
  - **Achtung!** `TIME` is not measured in seconds. (I have made this mistake
    at least twice.)

#### `/fade_out TIME`

- Fades the file `{name}` out gradually, over a period of `TIME` milliseconds.

#### `/debug`

- Spews a bunch of debug information about the file `{name}` to the logs.

## Other caveats

- Resampling is not supported; everything has to be the same sample rate (files,
  JACK)
- You can't specify custom volumes for things, which sucks
- Shutdown sucks
- Logging is okay but not stellar

