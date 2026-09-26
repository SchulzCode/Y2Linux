"""Early signed regulatory database from the same locked package as Y2ROOT.

cfg80211 is built in and requests these files before switch_root. No radio
firmware, private provisioning or network access is involved here.
"""
import hashlib
import io
import tarfile

VERSION = '2026.09.03'
# Pinned Buildroot 2025.02.18 package/wireless-regdb/wireless-regdb.hash.
SHA256 = 'b22e0901227b820cd1c280abe681a15b773a5103a5e10dc442e94ebb34cbf58d'
FILES = {
    'regulatory.db': 'lib/firmware/regulatory.db',
    'regulatory.db.p7s': 'lib/firmware/regulatory.db.p7s',
    'LICENSE': 'usr/share/licenses/wireless-regdb/LICENSE',
}


def load(project):
    archive = 'wireless-regdb-' + VERSION
    path = project/'.cache/buildroot-dl/wireless-regdb'/(archive+'.tar.xz')
    raw = path.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SHA256:
        raise ValueError('early regulatory archive hash mismatch')
    result = {}
    with tarfile.open(fileobj=io.BytesIO(raw), mode='r:xz') as source:
        for name, dest in FILES.items():
            entry = source.getmember(archive+'/'+name)
            if not entry.isfile() or not 0 < entry.size <= 65536:
                raise ValueError('early regulatory archive entry '+name)
            result[dest] = source.extractfile(entry).read()
    return result
