#!/bin/sh
set -eu
# One clean integrated build with the current baseline contracts. Historical
# per-screen/per-device fixtures are not the integrated candidate's test suite.
sh /project/tools/build/build.sh 'test_baseline*.py' test_pwrap.py \
    'test_usb_*.py' test_relay.py test_artifacts.py
cd /project
python3 -m unittest discover -s tests/observation -p 'test_usb_*.py' -v > /build/host-tests.log 2>&1
