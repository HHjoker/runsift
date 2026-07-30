#!/bin/sh
set -eu

log_path="$1"

printf '%s\n' \
  '[2026-07-30T10:00:00+08:00] [info] [thread 17] parser received 100 records' \
  '[2026-07-30T10:00:01+08:00] [error] [thread 17] invalid record length 18 at offset 8192' \
  '  at parser.cpp:42' \
  '[2026-07-30T10:00:02+08:00] [error] [thread 17] invalid record length 21 at offset 17664' \
  '[2026-07-30T10:00:03+08:00] [info] [thread 17] parser produced 98 records' \
  >> "$log_path"

printf '%s\n' \
  'Test project /tmp/build' \
  '1/1 Test #1: ParserTest.OptionalField ...***Failed' \
  'Expected: 100' \
  'Actual: 98'

printf '%s\n' 'ParserTest.OptionalField failed' >&2
exit 7
