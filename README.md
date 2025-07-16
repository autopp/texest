# texest

[![codecov](https://codecov.io/gh/autopp/texest/graph/badge.svg?token=TMBNHI2I9F)](https://codecov.io/gh/autopp/texest)

texest is testing framework for shell scripts, CLI tools, and any command-line application. It uses YAML files to declare test specifications and provides comprehensive assertion capabilities for command execution, output validation, and file system state verification.

## Features

- 📝 **YAML-based test definitions** - Write clear, readable test specifications
- 🚀 **Foreground and background process execution** - Test single commands or complex multi-process scenarios
- ✅ **Comprehensive assertions** - Validate exit status, stdout, stderr, and file contents
- 🔄 **Dynamic expressions** - Use environment variables, JSON/YAML data, and temporary resources
- ⏱️ **Wait conditions** - Synchronize processes with given condition (E.g. HTTP health check, stdout patterns)
- 🎯 **Flexible matchers** - Exact matching, regex patterns, JSON comparison, etc...
- 📊 **Multiple output formats** - Simple human-readable or structured JSON output

## Installation

Download the latest executable from [releases](https://github.com/autopp/texest/releases).

Or build from source:

```bash
git clone https://github.com/autopp/texest.git
cd texest
cargo build --release
# Binary will be at ./target/release/texest
```

## Quick Start

Create a test file `example.yaml`:

```yaml
tests:
  - name: test echo command
    command: [echo, "Hello, World!"]
    expect:
      status:
        eq: 0
      stdout:
        eq: "Hello, World!\n"
```

Run the test:

```bash
texest example.yaml
```

## Usage

```
Usage: texest [OPTIONS] [FILES]...

Arguments:
  [FILES]...         Test specification files (YAML)

Options:
      --color <COLOR>    Color output mode [default: auto] [possible values: auto, always, never]
      --format <FORMAT>  Output format [default: simple] [possible values: simple, json]
      --tee-stdout       Print stdout of commands during execution
      --tee-stderr       Print stderr of commands during execution
  -h, --help             Print help
```

## Test Specification

### Basic Structure

Tests are defined in YAML files with a `tests` array:

```yaml
tests:
  - name: optional test name
    command: [program, arg1, arg2]
    stdin: "input data"
    expect:
      status:
        eq: 0
      stdout:
        eq: "expected output"
      stderr:
        eq: ""
```

### Command Definition

Commands can be specified as arrays or use expressions:

```yaml
# Simple command
command: [ls, -la]

# Using environment variables
command: [$env: SHELL, -c, "echo hello"]

# With default values
command: [$env: SHELL-bash, -c, "echo hello"]
```

### Expressions

texest supports various expression types for dynamic values:

#### Environment Variables (`$env`)
```yaml
command: [$env: HOME, /.config/app]
# With default value
command: [$env: PORT-8080]
```

#### JSON Data (`$json`)
```yaml
stdin:
  $json:
    name: "test"
    value: 42
# Produces: {"name":"test","value":42}
```

#### YAML Data (`$yaml`)
```yaml
stdin:
  $yaml:
    tests:
      - command: [echo, test]
# Produces YAML-formatted string
```

#### Temporary Files (`$tmp_file`)
```yaml
command: [cat, {$tmp_file: "file content"}]
# Creates a temporary file with the specified content
```

#### Temporary Ports (`$tmp_port`)
```yaml
let:
  port: {$tmp_port: {}}
command: [serve, --port, {$var: port}]
```

### Assertions

#### Status Code
```yaml
expect:
  status:
    eq: 0  # Exact match
```

#### Output Streams
```yaml
expect:
  stdout:
    eq: "exact match\n"              # Exact string match
    match_regex: "pattern.*"         # Regular expression
    contain: "substring"             # Contains substring
    eq_json:                         # JSON comparison (ignores formatting)
      key: "value"
  stderr:
    not.eq: "error"                  # Negation with not. prefix
    not.match_regex: "error.*"
```

#### File Contents
```yaml
expect:
  files:
    /tmp/output.txt:
      eq: "expected content"
    /tmp/data.json:
      eq_json:
        status: "success"
```

### Background Processes

Run commands in the background with wait conditions:

```yaml
tests:
  - name: test server
    background:
      server:
        command: [python, -m, http.server, 8080]
        wait:
          http:
            url: http://localhost:8080
            timeout: 5s
    command: [curl, http://localhost:8080]
    expect:
      status:
        eq: 0
```

### Multiple Processes

Test multiple processes running concurrently:

```yaml
tests:
  - name: client-server test
    background:
      server:
        command: [./server, --port, 9000]
        wait:
          stdout:
            match_regex: "Server started"
      monitor:
        command: [./monitor, --port, 9000]
    command: [./client, --port, 9000, --message, "test"]
    expect:
      status:
        eq: 0
      stdout:
        contain: "success"
```

### Variables

Define reusable values with `let`:

```yaml
let:
  port: {$tmp_port: {}}
  base_url: "http://localhost"
tests:
  - command: [curl, "{$var: base_url}:{$var: port}"]
```

### Wait Conditions

#### Wait for Output
```yaml
wait:
  stdout:
    match_regex: "Ready"
  timeout: 10s
```

#### Wait for HTTP Endpoint
```yaml
wait:
  http:
    url: http://localhost:8080/health
    timeout: 5s
```

## Examples

### Testing a CLI Tool

```yaml
tests:
  - name: test CLI help
    command: [mytool, --help]
    expect:
      status:
        eq: 0
      stdout:
        contain: "Usage:"

  - name: test CLI with input file
    command: [mytool, process, {$tmp_file: "input data"}]
    expect:
      status:
        eq: 0
      files:
        output.txt:
          eq: "processed data"
```

### Testing a Web Service

```yaml
let:
  port: {$tmp_port: {}}
tests:
  - name: test API endpoint
    background:
      api:
        command: [./api-server, --port, {$var: port}]
        wait:
          http:
            url: "http://localhost:{$var: port}/health"
            timeout: 10s
    command: [curl, -X, POST, "http://localhost:{$var: port}/api/data",
              -H, "Content-Type: application/json",
              -d, {$json: {key: "value"}}]
    expect:
      status:
        eq: 0
      stdout:
        eq_json:
          status: "success"
          data:
            key: "value"
```

### Testing Shell Scripts

```yaml
tests:
  - name: test backup script
    command: [./backup.sh, /source, /dest]
    expect:
      status:
        eq: 0
      stdout:
        match_regex: "Backup completed: \\d+ files"
      files:
        /dest/backup.log:
          contain: "Success"
```

## Development

### Building from Source

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Run e2e tests
./e2e/run.sh
```

### Running E2E Tests

```bash
# Run all e2e tests
./e2e/run.sh
```

## License

[Apache-2.0](LICENSE)
