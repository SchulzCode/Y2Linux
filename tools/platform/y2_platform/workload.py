"""Scoped semantic QoS for platform workers; the kernel owns frequency policy."""
# SPDX-License-Identifier: GPL-2.0-only
import functools
import os
import threading


def workload(class_name):
    if class_name not in ('NetworkTransfer', 'Maintenance'):
        raise ValueError('unsupported_platform_workload')
    def decorate(function):
        @functools.wraps(function)
        def wrapped(ctx, *args, **kwargs):
            fd = None
            stopped = threading.Event()
            worker = None
            payload = (class_name + ' 3000\n').encode('ascii')
            try:
                try:
                    fd = os.open(ctx.path('/dev/y2-workload'), os.O_WRONLY | os.O_CLOEXEC)
                    def renew():
                        while not stopped.is_set():
                            try:
                                if os.write(fd, payload) != len(payload):
                                    return
                            except OSError:
                                return
                            if stopped.wait(1):
                                return
                    # Acquire before work starts; renew on an independent worker
                    # even when a blocking transfer subprocess runs for minutes.
                    if os.write(fd, payload) == len(payload):
                        worker = threading.Thread(target=renew, daemon=True)
                        worker.start()
                except OSError:
                    pass  # QoS is optional; never fail a transfer on a hint.
                return function(ctx, *args, **kwargs)
            finally:
                stopped.set()
                if worker is not None:
                    worker.join()
                if fd is not None:
                    os.close(fd)  # close/process death releases only this lease
        return wrapped
    return decorate
