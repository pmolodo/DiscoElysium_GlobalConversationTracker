#!/bin/bash

THIS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${THIS_DIR}"
git merge --no-edit main line_length_80
git merge --no-edit main line_length_120
git checkout main
