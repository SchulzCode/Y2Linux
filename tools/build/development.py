#!/usr/bin/env python3
"""Compatibility entrypoint: development now builds the production platform."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[2]))
from tools.production.build import main
if __name__ == '__main__':
    print('Development and release use tools/production/build.py; SD-root builds are retired.',flush=True)
    main()
