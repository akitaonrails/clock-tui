#!/usr/bin/env bash
set -euo pipefail

tests_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$tests_dir/../.." && pwd)
widget="$repo_root/examples/widgets/tclock-weather"

# shellcheck source=/dev/null
source "$widget"

fail() { printf 'FAIL %s\n' "$1" >&2; return 1; }

assert_eq() {
  local actual=$1 expected=$2 label=$3
  if [ "$actual" != "$expected" ]; then
    printf 'FAIL %s: expected [%s], got [%s]\n' "$label" "$expected" "$actual" >&2
    return 1
  fi
}

assert_eq "$(wmo_label 0)" 'Clear' 'wmo 0'
assert_eq "$(wmo_label 2)" 'Partly cloudy' 'wmo 2'
assert_eq "$(wmo_label 61)" 'Rain' 'wmo 61'
assert_eq "$(wmo_label 71)" 'Snow' 'wmo 71'
assert_eq "$(wmo_label 95)" 'Thunderstorm' 'wmo 95'
assert_eq "$(wmo_label 123)" 'Unknown' 'wmo unknown'

assert_eq "$(weekday_from_iso 2026-09-21)" '2' 'weekday monday'
assert_eq "$(weekday_from_iso 2000-01-01)" '0' 'weekday saturday'
assert_eq "$(weekday_name 2)" 'Mon' 'weekday name mon'
assert_eq "$(weekday_name 0)" 'Sat' 'weekday name sat'

assert_eq "$(round_number 22.4)" '22' 'round down'
assert_eq "$(round_number 26.7)" '27' 'round up'
assert_eq "$(round_number -3.6)" '-4' 'round negative'
assert_eq "$(round_number 70)" '70' 'round integer'
assert_eq "$(round_number '')" '0' 'round empty'

assert_eq "$(urlencode 'Porto Alegre')" 'Porto+Alegre' 'urlencode space'
assert_eq "$(urlencode 'Curitiba')" 'Curitiba' 'urlencode token'
assert_eq "$(urlencode 'São Paulo')" 'S%C3%A3o+Paulo' 'urlencode utf8 two-byte'
assert_eq "$(urlencode '東京')" '%E6%9D%B1%E4%BA%AC' 'urlencode utf8 three-byte'

mtime_probe=$(mktemp)
mtime_value=$(file_mtime "$mtime_probe")
[[ "$mtime_value" =~ ^[0-9]+$ ]] || fail "file_mtime not numeric: [$mtime_value]"
rm -f "$mtime_probe"

run_script() { "$widget" "$@" 2>&1; }

expect_fail() {
  local label=$1 expected=$2
  shift 2
  local out
  out=$(run_script "$@" || true)
  if run_script "$@" >/dev/null 2>&1; then
    fail "$label: expected non-zero exit"
    return 1
  fi
  if [[ "$out" != *"$expected"* ]]; then
    printf 'FAIL %s: expected [%s], got [%s]\n' "$label" "$expected" "$out" >&2
    return 1
  fi
}

assert_eq "$(run_script --help)" "$(usage)" 'help text'
assert_eq "$(run_script -h)" "$(usage)" 'short help text'
expect_fail 'no location' 'no location' --city ''
expect_fail 'unknown option' "unknown option '--bogus'" --bogus
expect_fail 'bad days' "invalid --days 'abc'" --city Curitiba --days abc
expect_fail 'zero days' "invalid --days '0'" --city Curitiba --days 0
expect_fail 'lon only' 'both --lat and --lon' --lon 1
expect_fail 'bad units' "invalid --units 'kelvin'" --city Curitiba --units kelvin
expect_fail 'bad lat' "invalid --lat 'x'" --lat x --lon 1
expect_fail 'missing value' '--city requires a value' --city

fixtures="$tests_dir/fixtures/weather"
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT
mock_bin="$test_tmp/bin"
mkdir -p "$mock_bin"

cat >"$mock_bin/curl" <<'MOCK'
#!/usr/bin/env bash
url=""
for a in "$@"; do url=$a; done
log=${WEATHER_CALL_LOG:?WEATHER_CALL_LOG not set}
printf '%s\n' "$url" >>"$log"
if [ "${WEATHER_FAIL_CURL:-0}" = 1 ]; then exit 7; fi
case "$url" in
  *geocoding-api.open-meteo.com*)
    case "${WEATHER_GEOCODE:-ok}" in
      empty) cat "$WEATHER_FIXTURES/geocode-empty.json" ;;
      malformed) cat "$WEATHER_FIXTURES/malformed.json" ;;
      *) cat "$WEATHER_FIXTURES/geocode-ok.json" ;;
    esac
    ;;
  *api.open-meteo.com*)
    case "${WEATHER_FORECAST:-ok}" in
      malformed) cat "$WEATHER_FIXTURES/malformed.json" ;;
      imperial) cat "$WEATHER_FIXTURES/forecast-imperial.json" ;;
      *) cat "$WEATHER_FIXTURES/forecast-metric.json" ;;
    esac
    ;;
  *) exit 22 ;;
esac
MOCK
chmod +x "$mock_bin/curl"
export WEATHER_FIXTURES="$fixtures"

run_widget() {
  WEATHER_CALL_LOG="${WEATHER_CALL_LOG_OVERRIDE:-$test_tmp/calls.log}" \
    XDG_CACHE_HOME="$test_tmp/cache" \
    PATH="$mock_bin:$PATH" \
    "$widget" "$@"
}

plain() { sed -E $'s/\x1b\\[[0-9;]*m//g'; }

json_out=$(run_widget --json --city Curitiba)
if [[ "$json_out" != *'"name": "Curitiba"'* ]]; then fail "json city: $json_out"; fi
if [[ "$json_out" != *'"temperature_2m": 22.4'* ]]; then fail "json temp: $json_out"; fi
if [[ "$json_out" != *'"weekday": ["Today", "Mon", "Tue"]'* ]]; then fail "json weekday: $json_out"; fi

calls="$test_tmp/lonely.log"
: >"$calls"
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --json --lat -25.42 --lon -49.27 >/dev/null
assert_eq "$(wc -l <"$calls" | tr -d ' ')" '1' 'lat/lon skips geocoding'

geocode_empty=$(WEATHER_GEOCODE=empty run_widget --no-cache --json --city Nowhere 2>&1 || true)
if [[ "$geocode_empty" != *'not found'* ]]; then fail "geocode empty: $geocode_empty"; fi

net_fail=$(WEATHER_FAIL_CURL=1 run_widget --no-cache --json --city Curitiba 2>&1 || true)
if [[ "$net_fail" != *'could not reach Open-Meteo geocoding'* ]]; then fail "network: $net_fail"; fi

bad=$(WEATHER_FORECAST=malformed run_widget --no-cache --json --city Curitiba 2>&1 || true)
if [[ "$bad" != *'unexpected response from Open-Meteo'* ]]; then fail "malformed: $bad"; fi

metric=$(run_widget --city Curitiba | plain)
[[ "$metric" == *'22°C'* ]] || fail "metric temp: $metric"
[[ "$metric" == *'feels 23°C · Partly cloudy'* ]] || fail "metric feels: $metric"
[[ "$metric" == *'Partly cloudy'* ]] || fail "metric cond: $metric"
[[ "$metric" == *'Humidity 70%'* ]] || fail "metric humidity: $metric"
[[ "$metric" == *'Wind 12 km/h'* ]] || fail "metric wind: $metric"
[[ "$metric" == *'Today'* ]] || fail "metric today: $metric"
[[ "$metric" == *'24°/17°'* ]] || fail "metric daily: $metric"

imperial=$(WEATHER_FORECAST=imperial run_widget --city Curitiba --units imperial | plain)
[[ "$imperial" == *'72°F'* ]] || fail "imperial temp: $imperial"
[[ "$imperial" == *'Wind 8 mph'* ]] || fail "imperial wind: $imperial"

current_only=$(run_widget --city Curitiba --current-only | plain)
[[ "$current_only" != *'Today'* ]] || fail "current-only leaked daily: $current_only"
[[ "$current_only" == *'22°C'* ]] || fail "current-only temp: $current_only"

default_raw=$(run_widget --city Curitiba)
[[ "$default_raw" == *$'\033[1;36m'* ]] || fail "default theme title code missing"
nerv_raw=$(TCLOCK_WIDGET_THEME=nerv run_widget --city Curitiba)
[[ "$nerv_raw" == *$'\033[1;31m'* ]] || fail "nerv theme title code missing"
unknown_raw=$(TCLOCK_WIDGET_THEME=whatever run_widget --city Curitiba)
[[ "$unknown_raw" == *$'\033[1;36m'* ]] || fail "unknown theme should fall back to default"
no_color_raw=$(run_widget --city Curitiba --no-color)
[[ "$no_color_raw" != *$'\033['* ]] || fail "no-color still emitted escapes"

calls="$test_tmp/cache.log"
rm -rf "$test_tmp/cache"
: >"$calls"
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --city Curitiba >/dev/null
assert_eq "$(wc -l <"$calls" | tr -d ' ')" '2' 'first run makes geocode+forecast calls'
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --city Curitiba >/dev/null
assert_eq "$(wc -l <"$calls" | tr -d ' ')" '2' 'second run within TTL uses cache'

rm -rf "$test_tmp/cache"
: >"$calls"
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --no-cache --city Curitiba >/dev/null
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --no-cache --city Curitiba >/dev/null
assert_eq "$(wc -l <"$calls" | tr -d ' ')" '4' 'no-cache always hits network'

rm -rf "$test_tmp/cache"
: >"$calls"
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --cache-secs 0 --city Curitiba >/dev/null
WEATHER_CALL_LOG_OVERRIDE="$calls" run_widget --cache-secs 0 --city Curitiba >/dev/null
assert_eq "$(wc -l <"$calls" | tr -d ' ')" '4' 'cache-secs 0 always hits network'

printf 'weather widget scenarios passed\n'
