#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
widget="$repo_root/examples/widgets/tclock-system-health"
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT

mock_bin="$test_tmp/bin"
mkdir -p "$mock_bin"

cat >"$mock_bin/mock-command" <<'MOCK'
#!/usr/bin/env bash
set -u

command_name=${0##*/}
scenario=${HEALTH_SCENARIO:?}

has_arg() {
  local wanted=$1 arg
  shift
  for arg; do
    [ "$arg" = "$wanted" ] && return 0
  done
  return 1
}

case "$command_name" in
  systemctl)
    scope=system
    [ "${1-}" = --user ] && { scope=user; shift; }

    if [ "${1-}" = list-timers ]; then
      exit 0
    fi

    if [ "${1-}" = --failed ]; then
      # a broken systemctl: nonzero with no output, so the widget must say
      # "unknown" instead of claiming zero failed units
      [ "$scenario" = broken ] && exit 1
      if [ "$scope" = system ] && { [ "$scenario" = retained ] || [ "$scenario" = unavailable ] || [ "$scenario" = mixed ]; }; then
        printf 'mnt-data.automount loaded failed failed Data automount\n'
        [ "$scenario" = mixed ] && printf 'backup.service loaded failed failed Backup service\n'
      fi
      exit 0
    fi

    if [ "${1-}" = show ]; then
      unit=${2-}
      shift 2
      if has_arg --value "$@"; then
        case "$unit:$*" in
          mnt-data.mount:*ActiveState*)
            { [ "$scenario" = retained ] || [ "$scenario" = mixed ]; } && printf 'active\n' || printf 'inactive\n' ;;
          mnt-data.mount:*SubState*)
            { [ "$scenario" = retained ] || [ "$scenario" = mixed ]; } && printf 'mounted\n' || printf 'dead\n' ;;
          mnt-data.mount:*Result*)
            { [ "$scenario" = retained ] || [ "$scenario" = mixed ]; } && printf 'success\n' || printf 'mount-failed\n' ;;
          *NextElapseUSecRealtime*|*LastTriggerUSec*) printf 'n/a\n' ;;
          *) printf '\n' ;;
        esac
        exit 0
      fi

      case "$unit" in
        pipewire|wireplumber|xdg-desktop-portal|NetworkManager|bluetooth)
          load=loaded; active=active
          case "$scenario" in
            svcnone) load=not-found ;;
            svcfailed)
              [ "$unit" = NetworkManager ] && active=failed
              [ "$unit" = bluetooth ] && load=not-found ;;
            svcstale)
              [ "$unit" = bluetooth ] && active=inactive ;;
            svcunknown)
              [ "$unit" = pipewire ] && exit 0 ;;
          esac
          if [ "$load" = not-found ]; then
            printf 'LoadState=not-found\nUnitFileState=\nActiveState=inactive\n'
          else
            printf 'LoadState=%s\nUnitFileState=enabled\nActiveState=%s\n' "$load" "$active"
          fi
          exit 0 ;;
        mnt-data.automount)
          printf '%s\n' \
            'Description=Data automount' \
            'Result=start-limit-hit' \
            'ActiveState=failed' \
            'SubState=dead' \
            'StateChangeTimestamp=Mon 2026-08-20 11:53:37 -03' \
            'ExecMainCode=' \
            'ExecMainStatus=' ;;
        mnt-data.mount)
          if [ "$scenario" = retained ] || [ "$scenario" = mixed ]; then
            printf '%s\n' 'Result=success' 'ActiveState=active' 'SubState=mounted'
          else
            printf '%s\n' 'Result=mount-failed' 'ActiveState=inactive' 'SubState=dead'
          fi ;;
        backup.service)
          printf '%s\n' \
            'Description=Backup service' \
            'Result=exit-code' \
            'ActiveState=failed' \
            'SubState=failed' \
            'StateChangeTimestamp=Mon 2026-08-25 10:00:00 -03' \
            'ExecMainCode=exited' \
            'ExecMainStatus=1' ;;
        *)
          # real systemd answers not-found for units that are not installed
          printf 'LoadState=not-found\nUnitFileState=\nActiveState=inactive\n' ;;
      esac
      exit 0
    fi
    ;;
  journalctl)
    printf 'Mount request failed because the network was unreachable\n'
    ;;
  checkupdates)
    case "$scenario" in
      updates)
        printf 'alpha 1.0-1 -> 1.1-1\nbeta 2.0-1 -> 2.1-1\ngamma 3.0-1 -> 3.0.1-1\n' ;;
      updatesfail)
        printf 'error: failed to synchronise any databases\n'; exit 1 ;;
      *) exit 2 ;;
    esac
    ;;
  df)
    case "$scenario" in
      full) pct=96; size=1000; used=960; avail=40 ;;
      balanced) pct=50; size=1000; used=500; avail=500 ;;
      *) pct=10; size=1000; used=100; avail=900 ;;
    esac
    if [[ "$*" = *source,size,used,avail,pcent,target* ]]; then
      printf 'Filesystem 1B-blocks Used Available Use%% Mounted on\n'
      printf '/dev/test %s %s %s %s%% /data\n' "$size" "$used" "$avail" "$pct"
    elif [[ "$*" = *source,pcent,target* ]]; then
      printf 'Filesystem Use%% Mounted on\n/dev/test %s%% /data\n' "$pct"
    elif [[ "$*" = *output=pcent* ]]; then
      printf 'Use%%\n%s%%\n' "$pct"
    fi
    ;;
  findmnt)
    if [ "$scenario" = full ] || [ "$scenario" = balanced ]; then
      printf '/data /dev/test\n'
    fi
    ;;
  systemd-escape)
    printf 'data\n'
    ;;
  btrfs)
    case "${1-} ${2-}" in
      'device stats')
        printf '%s\n' \
          '[/dev/test].write_io_errs    0' \
          '[/dev/test].read_io_errs     0' \
          '[/dev/test].flush_io_errs    0' \
          '[/dev/test].corruption_errs  0' \
          '[/dev/test].generation_errs  0' ;;
      'filesystem usage')
        if [ "$scenario" = balanced ]; then free=500; used=500; else free=40; used=960; fi
        printf '%s\n' \
          '    Device size:                 1000' \
          '    Device allocated:             980' \
          '    Device unallocated:            20' \
          "    Used:                         $used" \
          "    Free (estimated):             $free" \
          "    Free (statfs, df):             $free" ;;
    esac
    ;;
  ps)
    exit 0
    ;;
  nproc)
    printf '4\n'
    ;;
  du)
    printf '600\t/data/big\n200\t/data/small\n800\t/data\n'
    ;;
esac
MOCK
chmod +x "$mock_bin/mock-command"
for command_name in systemctl journalctl df findmnt systemd-escape btrfs ps nproc du checkupdates; do
  ln -s mock-command "$mock_bin/$command_name"
done

meminfo_normal="$test_tmp/meminfo-normal"
printf '%s\n' 'MemTotal:       1000000 kB' 'MemAvailable:    900000 kB' \
  'SwapTotal:      1000000 kB' 'SwapFree:        900000 kB' >"$meminfo_normal"
meminfo_pressure="$test_tmp/meminfo-pressure"
printf '%s\n' 'MemTotal:       1000000 kB' 'MemAvailable:     10000 kB' \
  'SwapTotal:      1000000 kB' 'SwapFree:         10000 kB' >"$meminfo_pressure"
meminfo_partial="$test_tmp/meminfo-partial"
printf '%s\n' 'MemTotal:       1000000 kB' >"$meminfo_partial"

plain_output() {
  sed -E $'s/\x1b\\[[0-9;]*m//g'
}

run_widget() {
  HEALTH_SCENARIO=$1 PATH="$mock_bin:$PATH" "$widget" "${@:2}" | plain_output
}

assert_contains() {
  local output=$1 expected=$2
  if [[ "$output" != *"$expected"* ]]; then
    printf 'expected output to contain: %s\n\n%s\n' "$expected" "$output" >&2
    return 1
  fi
}

assert_not_contains() {
  local output=$1 unexpected=$2
  if [[ "$output" = *"$unexpected"* ]]; then
    printf 'expected output not to contain: %s\n\n%s\n' "$unexpected" "$output" >&2
    return 1
  fi
}

retained=$(run_widget retained --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$retained" 'attention needed'
assert_contains "$retained" '1 retained (0u/1s)'
assert_not_contains "$retained" 'problems detected'

unavailable=$(run_widget unavailable --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$unavailable" 'problems detected'
assert_contains "$unavailable" '1 failed (0u/1s)'
assert_not_contains "$unavailable" 'retained'

mixed=$(run_widget mixed --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$mixed" 'problems detected'
assert_contains "$mixed" '1 failed (0u/1s)'
assert_contains "$mixed" '1 retained (0u/1s)'

full=$(run_widget full --grub-btrfs-cfg /nonexistent --single-column)
assert_contains "$full" 'storage  data 96%'
assert_contains "$full" 'alloc data capacity-bound'
assert_not_contains "$full" 'alloc data 2% unalloc'

balanced=$(run_widget balanced --grub-btrfs-cfg /nonexistent --single-column)
assert_contains "$balanced" 'storage  data 50%'
assert_contains "$balanced" 'alloc data 2% unalloc'
assert_contains "$balanced" 'problems detected'

details=$(run_widget retained --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$details" 'attention needed · 1 warning'
assert_contains "$details" 'jobs      1 automount unit(s) still show an old failure although the mount recovered'
assert_contains "$details" 'mnt-data.automount [system · retained failed state]'
assert_contains "$details" 'mnt-data.mount is active/mounted with result success'
assert_not_contains "$details" 'Nothing is flagged'

details=$(run_widget full --details)
assert_contains "$details" 'problems detected · 1 critical'
assert_contains "$details" 'storage   /data is 96% full'
assert_contains "$details" '/data — 96% used (960 B / 1000 B, 40 B free)'
assert_contains "$details" '/data/big — 600 B'
assert_contains "$details" 'Low unallocated device space is a consequence of filesystem capacity pressure.'
assert_contains "$details" 'Delete or move data first; balancing does not create free space.'

# balanced: the only problem is btrfs allocation — the summary must say so
details=$(run_widget balanced --details)
assert_contains "$details" 'problems detected · 1 critical'
assert_contains "$details" 'btrfs     /data has only 2% of its device unallocated'
assert_contains "$details" '/data — 50% used · 2% unallocated (20 B of 1000 B)'
assert_contains "$details" 'Inspect chunk usage before considering a targeted balance.'
assert_contains "$details" 'sudo btrfs balance start -dusage=50 /data'

# a missing grub-btrfs.cfg is explained, not silently dropped from details
details=$(run_widget balanced --details --snapshots --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$details" 'attention needed · 1 warning'
assert_contains "$details" 'snapshot  grub-btrfs.cfg is missing or unreadable at /nonexistent'
assert_contains "$details" 'Timeshift snapshots'
assert_contains "$details" '/nonexistent is missing or not readable by this user.'

# healthy: no findings, and no stale "problem" text
details=$(run_widget healthy --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$details" 'all systems healthy'
assert_contains "$details" 'Nothing is flagged'
assert_not_contains "$details" 'problems detected'

# services: a failed daemon is critical; an uninstalled one is silently absent
svcfailed=$(run_widget svcfailed --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$svcfailed" 'problems detected'
assert_contains "$svcfailed" 'network failed'
assert_contains "$svcfailed" 'audio ok'
assert_not_contains "$svcfailed" 'bt'

svc_details=$(run_widget svcfailed --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$svc_details" 'NetworkManager is failed'
assert_contains "$svc_details" 'Services'
assert_contains "$svc_details" 'network    failed'

# every default service missing: unknown, never a silent "all healthy"
svcnone=$(run_widget svcnone --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$svcnone" 'all systems healthy'
assert_contains "$svcnone" 'none installed'
svcnone_details=$(run_widget svcnone --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$svcnone_details" 'None of the configured services are installed on this host — nothing to check.'

# an unreadable unit is unknown, never ok
svcunknown=$(run_widget svcunknown --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$svcunknown" 'attention needed'
assert_contains "$svcunknown" 'audio unknown'
svcunknown_details=$(run_widget svcunknown --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$svcunknown_details" 'pipewire (state could not be read)'

# installed but inactive is informational: the host decided, it is not a fault
svcstale=$(run_widget svcstale --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$svcstale" 'bt off'
assert_contains "$svcstale" 'all systems healthy'

noservices=$(run_widget healthy --no-services --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_not_contains "$noservices" 'services'

customservice=$(run_widget healthy --service custom:system:backup-daemon \
  --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_not_contains "$customservice" 'audio'
assert_contains "$customservice" 'none installed'
assert_contains "$customservice" 'all systems healthy'

# memory and swap pressure raise the verdict; unreadable memory is "unknown"
mempress=$(run_widget healthy --meminfo "$meminfo_pressure" --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$mempress" 'problems detected'
assert_contains "$mempress" 'mem 99%'
assert_contains "$mempress" 'swap 99%'
mem_details=$(run_widget healthy --meminfo "$meminfo_pressure" --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$mem_details" 'memory is 99% in use'
assert_contains "$mem_details" 'swap is 99% full'
assert_contains "$mem_details" 'swap storm'

memunknown=$(run_widget healthy --meminfo "$meminfo_partial" --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$memunknown" 'attention needed'
assert_contains "$memunknown" 'mem ?%'
memunknown_details=$(run_widget healthy --meminfo "$meminfo_partial" --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$memunknown_details" 'memory usage could not be read'

# pending updates are informational by default, warnings only above --updates-warn
updates=$(run_widget updates --updates --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$updates" '3 updates'
assert_contains "$updates" 'all systems healthy'

updateswarn=$(run_widget updates --updates-warn 2 --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$updateswarn" 'attention needed'
updates_details=$(run_widget updates --updates-warn 2 --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$updates_details" '3 package updates are pending'

updatesfail=$(run_widget updatesfail --updates --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$updatesfail" 'attention needed'
assert_contains "$updatesfail" 'updates unknown'

updatesoff=$(run_widget updates --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_not_contains "$updatesoff" 'updates'

# a systemctl that fails must not read as zero failed units
broken=$(run_widget broken --meminfo "$meminfo_normal" --grub-btrfs-cfg /nonexistent --no-btrfs --single-column)
assert_contains "$broken" 'attention needed'
assert_contains "$broken" 'unknown'
broken_details=$(run_widget broken --meminfo "$meminfo_normal" --details --grub-btrfs-cfg /nonexistent --no-btrfs)
assert_contains "$broken_details" 'failed-unit state could not be read'

printf 'system-health widget scenarios passed\n'
