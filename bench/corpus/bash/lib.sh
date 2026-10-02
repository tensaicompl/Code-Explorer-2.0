# Sourced helpers.
join_by() {
  local IFS="$1"
  shift
  echo "$*"
}

retry() {
  local attempts="$1"; shift
  local n=0
  until "$@"; do
    n=$((n + 1))
    if (( n >= attempts )); then
      return 1
    fi
    sleep $(( n * 2 ))
  done
}

select_first() {
  local -n arr=$1
  echo "${arr[0]:-}"
}
