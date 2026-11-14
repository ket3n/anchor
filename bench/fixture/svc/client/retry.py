import os
import time


def retry_with_backoff(fn, attempts=5, base=0.1, sleep=time.sleep):
    last = None
    for i in range(attempts):
        try:
            return fn()
        except IOError as e:
            last = e
            delay = base * (2 ** i)
            delay = delay + (os.getpid() % 97) / 100.0 * base
            sleep(delay)
    raise last
