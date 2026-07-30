#!/bin/sh
set -eu

log_path="$1"
test_report_path="$2"

printf '%s\n' \
  '2026-07-30 10:00:00.100 | info | 17 | parser | received batch 4821' \
  '2026-07-30 10:00:00.200 | error | 17 | parser | record buffer was already freed' \
  >> "$log_path"

printf '%s\n' \
  '<?xml version="1.0"?>' \
  '<testsuites tests="1" failures="1" name="AllTests">' \
  '  <testsuite name="ParserTest" tests="1" failures="1">' \
  '    <testcase name="RejectsFreedBuffer" status="run" result="completed" time="0.012">' \
  '      <failure message="heap-use-after-free">parser returned corrupted data</failure>' \
  '    </testcase>' \
  '  </testsuite>' \
  '</testsuites>' \
  > "$test_report_path"

printf '%s\n' \
  '==42==ERROR: AddressSanitizer: heap-use-after-free on address 0x1234' \
  '    #0 0x1000 in parse_record /work/parser.cpp:42' \
  '    #1 0x2000 in main /work/main.cpp:10' \
  'SUMMARY: AddressSanitizer: heap-use-after-free' \
  >&2

exit 7
