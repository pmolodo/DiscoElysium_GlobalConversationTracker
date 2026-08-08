#!/bin/bash

THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${THIS_DIR}"
git checkout line_length_80
git merge --no-edit main
git checkout line_length_120
git merge --no-edit main
git checkout main
