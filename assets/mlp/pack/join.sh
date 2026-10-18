#!/bin/sh
# Склеивает пак пони из частей / Joins the pony pack from its parts.
cd "$(dirname "$0")"
cat all_v15c.mlpo.part00 all_v15c.mlpo.part01 > all_v15c.mlpo
echo "expected 1f7195783148bd67a676edaafa458f872cbc6fb5b7e0eac27fcd9c9234565c31"
sha256sum all_v15c.mlpo
