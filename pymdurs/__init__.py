# Import the Rust extension module
import sys

from . import pymdurs as _native
from .pymdurs import *

# `from .pymdurs import *` does not re-export extension submodules, so
# `import pymdurs.geometric` / `from pymdurs.geometric import …` would fail.
for _name in ("geometric", "thermal"):
    _sub = getattr(_native, _name, None)
    if _sub is not None:
        globals()[_name] = _sub
        sys.modules.setdefault(f"{__name__}.{_name}", _sub)

# Pure-Python helpers (tree extraction from CHM); keep separate from Rust geometric.
from . import trees as trees  # noqa: E402
