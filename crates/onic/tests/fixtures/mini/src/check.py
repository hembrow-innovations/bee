from src.hash import hash_password, hasher  # noqa: F401

from .hash import hash_password, hasher  # noqa: F401


def run_imported(password): return hasher(password)


def run_imported_miss(password): return hasher21(password)
