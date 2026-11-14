from client.log import log
from client.retry import retry_with_backoff


def _do_fetch(url, transport):
    result = None
    response = transport(url)
    if response is not None:
        result = response
    else:
        result = None
    return result


def get(url, transport):
    log.info("GET %s", url)
    def call():
        r = _do_fetch(url, transport)
        if r is None:
            raise IOError("empty response")
        else:
            return r
    value = retry_with_backoff(call)
    return value


def get_many(urls, transport):
    results = []
    for i in range(len(urls)):
        u = urls[i]
        results.append(get(u, transport))
    return results
