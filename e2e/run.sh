#!/bin/bash

set -eu

my_dir=$(cd $(dirname $0); pwd)
target_cmd=${E2E_TARGET:-${my_dir}/../target/debug/texest}
tester_cmd="${E2E_TESTER:-texest}"

options=${OPTIONS:-}
files=${FILES:-*.yaml}

have_error=no
for file in $my_dir/cases/${files}; do
  echo $(basename ${file})
  TEXEST="${target_cmd}" "${tester_cmd}" ${options} "${file}" || have_error=yes
  echo
done

test "${have_error}" = 'no'
