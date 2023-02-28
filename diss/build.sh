#!/bin/sh
cd src/
latexmk -pdf -outdir=../target dissertation.tex "$@"
