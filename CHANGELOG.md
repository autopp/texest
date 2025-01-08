# CHANGELOG

## v0.9.0

- Add `$tmp_port` expr notation for reserve random port
- Make body of `$json` and `$yaml` to be expression
- Add `let` block for define variables and add `$var` expr notation
- Support negative matching by `not.*` notation
- Fix tee mode to print output of background process

## v0.8.0

- Add `--tee-stdout` and `--tee-stderr` options to enable tee mode from command line
- Format output of tee mode
- Add background process waiting condition `stream`
- Add more error handling (by reducing `unwrap()`)
- Remove codes for test from release build

## v0.7.0

- Add stream matcher `match_regex`

## v0.6.0

- Add expr notation for wait condition parameter
- Add default value notation to `$env` expr

## v0.5.0

- Add stream matcher `include_json`

## v0.4.0

- Add expectation of file
- Support waiting background process by condition (`sleep` or `http`)
- Add duration notation
- Add multiple process mode in a test case
- Add `$tmp_file` expr notation
- Improve message of timeout

## v0.3.0

- Reduce memory copy about of `String`
- Improve message of stream matcher `eq`
- Improve message of stream matcher `contain`
- Improve message of stream matcher `eq_json`

## v0.2.0

- Add expr notation for matcher parameter
- Add `--format` option for switch report format (`simple` or `json`)
- Add '$json' expr notation
- Add stream matcher `eq_json`
- Add test name
- Report failured test message
- Add stream matcher `contain`

## v0.1.0

- Initial release
