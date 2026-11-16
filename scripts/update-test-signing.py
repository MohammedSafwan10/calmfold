"""Ephemeral fixture signing using OpenSSL; never accesses a Keychain or CI secret."""
import base64
import hashlib
import os
from pathlib import Path
import subprocess

def create(directory: Path):
    seed = os.urandom(32)
    der = directory / "fixture-key.der"
    der.write_bytes(bytes.fromhex("302e020100300506032b657004220420") + seed)
    der.chmod(0o600)
    public_der = subprocess.check_output(["openssl", "pkey", "-inform", "DER", "-in", str(der), "-pubout", "-outform", "DER"])
    assert public_der[:12] == bytes.fromhex("302a300506032b6570032100")
    key_id = os.urandom(8)
    public = base64.b64encode(b"Ed" + key_id + public_der[12:]).decode()
    def sign(data):
        message = directory / "sign-input"
        message.write_bytes(data)
        signature = subprocess.check_output(["openssl", "pkeyutl", "-sign", "-keyform", "DER", "-inkey", str(der), "-rawin", "-in", str(message)])
        assert len(signature) == 64
        return signature
    def minisign(path):
        signature = sign(hashlib.blake2b(path.read_bytes()).digest())
        comment = b"calmfold disposable fixture"
        global_sig = sign(signature + comment)
        Path(str(path) + ".minisig").write_text("untrusted comment: fixture\n" + base64.b64encode(b"ED" + key_id + signature).decode() + "\ntrusted comment: " + comment.decode() + "\n" + base64.b64encode(global_sig).decode() + "\n")
    return public, minisign
