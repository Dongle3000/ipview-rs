"""Minimal SFTP server for ipview's integration tests.

Usage: python test_sftp_server.py ROOT HOST_KEY_FILE
Serves ROOT (so ROOT/usr4/account appears as /usr4/account), accepts user
"adfexc" with password "secret", listens on a free port on 127.0.0.1 and
prints that port on stdout. Requires the asyncssh package.
"""

import asyncio
import os
import sys

import asyncssh


class Server(asyncssh.SSHServer):
    def begin_auth(self, username):
        return True

    def password_auth_supported(self):
        return True

    def validate_password(self, username, password):
        return username == "adfexc" and password == "secret"


async def main(root, key_file):
    if not os.path.exists(key_file):
        asyncssh.generate_private_key("ssh-rsa").write_private_key(key_file)
    server = await asyncssh.listen(
        "127.0.0.1",
        0,
        server_host_keys=[key_file],
        server_factory=Server,
        sftp_factory=lambda chan: asyncssh.SFTPServer(chan, chroot=root.encode()),
    )
    print(server.sockets[0].getsockname()[1], flush=True)
    await asyncio.Future()


if __name__ == "__main__":
    asyncio.run(main(sys.argv[1], sys.argv[2]))
