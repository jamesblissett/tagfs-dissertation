#!/bin/sh

tagfs="target/debug/tagfs"

"${tagfs}" tag "/media/hdd/film/Before Sunrise (1995)" "genre=romance"
"${tagfs}" tag "/media/hdd/film/Before Sunrise (1995)" "genre=slice-of-life"
"${tagfs}" tag "/media/hdd/film/Before Sunset (2004)"  "genre=romance"
"${tagfs}" tag "/media/hdd/film/Casino (1995)"         "genre=crime"
"${tagfs}" tag "/media/hdd/film/Heat (1995)"           "genre=crime"
"${tagfs}" tag "/media/hdd/film/Before Sunrise (1995)" "favourite"

"${tagfs}" tag "/media/hdd/film/Before Sunrise (1995)" "actor=Julie\ Delpy"
