# texest

[![codecov](https://codecov.io/gh/autopp/texest/graph/badge.svg?token=TMBNHI2I9F)](https://codecov.io/gh/autopp/texest)

texest is testing framework for shell script, CLI tool, or any command.

## Features

- Declare specification of command by YAML
- Execute commands as foreground or background
- Assert exit status, stdout, stderr of the command
- Assert file content after

## Install

Download executable from [releases](https://github.com/autopp/texest/releases).

## Usage

```
Usage: texest [OPTIONS] [FILES]...

Arguments:
  [FILES]...

Options:
      --color <COLOR>    [default: auto] [possible values: auto, always, never]
      --format <FORMAT>  [default: simple] [possible values: simple, json]
  -h, --help             Print help
```

## License

[Apache-2.0](LICENSE)
