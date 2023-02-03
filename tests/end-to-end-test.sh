#!/bin/bash

# These variables are global even though they are defined in a function.
create_files() {
    tmp_db="$(mktemp || exit 1)"
    tmp_tree="$(mktemp -d || exit 1)"
    tmp_mount_point="$(mktemp -d || exit 1)"

    mkdir -p "${tmp_tree}/film" || exit 1
    mkdir -p "${tmp_tree}/film/Before Sunrise (1995)" || exit 1
    mkdir -p "${tmp_tree}/film/Before Sunset (2004)" || exit 1
    mkdir -p "${tmp_tree}/film/True Romance (1993)" || exit 1
    mkdir -p "${tmp_tree}/film/Heat (1995)" || exit 1
    mkdir -p "${tmp_tree}/film/Casino (1995)" || exit 1
}

tagfs() {
    target/debug/tagfs --database "${tmp_db}" "$@"
}

tagfs_mount() {
    # cannot use the helper function to call mount because it obscures the PID
    # making it difficult to kill the mount.
    target/debug/tagfs --database "${tmp_db}" mount "${tmp_mount_point}" > /dev/null 2>&1 &
    pid="$!"
    disown

    printf "%s\n" "${pid}"
}

# $1 is pid.
cleanup() {
    kill -TERM "${pid}"

    rm -r "${tmp_tree}"
    rmdir "${tmp_mount_point}"
    rm "${tmp_db}"
}

# $1 is path to test.
assert_exists() {
    if [ ! -e "$1" ]; then
        printf "assert_exists failed: path \"%s\" does not exist.\n" "$1" 1>&2
        exit 1
    fi
}

# $1 is path to test.
assert_not_exists() {
    if [ -e "$1" ]; then
        printf "assert_not_exists failed: path \"%s\" exists.\n" "$1" 1>&2
        exit 1
    fi
}

create_files

pid="$(tagfs_mount)"

tagfs tag "${tmp_tree}/film/Before Sunrise (1995)" 'genre=romance'
tagfs tag "${tmp_tree}/film/Before Sunset (2004)" 'genre=romance'
tagfs tag "${tmp_tree}/film/Before Sunrise (1995)" 'genre=slice-of-life'
tagfs tag "${tmp_tree}/film/True Romance (1993)" 'genre=romance'
tagfs tag "${tmp_tree}/film/True Romance (1993)" 'genre=crime'
tagfs tag "${tmp_tree}/film/Casino (1995)" 'genre=crime'
tagfs tag "${tmp_tree}/film/Heat (1995)" 'genre=crime'

assert_exists "${tmp_mount_point}/?/genre=romance and genre=crime/True Romance (1993)"

if [ "$(tagfs query --case-sensitive 'genre=ROMaNce' 2> /dev/null)" ]; then
    printf "%s: fail: query returned rows when shouldn't have.\n" "${LINENO}" 1>&2
    exit 1
fi

cleanup "${pid}"
