# Unix command guardian prototype

This component is not enabled by the coding worker or shipped in app bundles yet.
A worker launches the guardian with a dedicated stdin pipe and retains its write
end. The guardian places the command in a separate process group; the command's
stdin is /dev/null. EOF or an error on the liveness pipe kills the owned group,
including descendants. Normal completion also cleans up remaining descendants.
The command leader is observed with waitid/WNOWAIT and reaped only after cleanup,
so its reserved PID cannot be reused to target another process group.

The ready file is a prototype handshake for tests, not a trusted recovery journal.
Production integration needs an authenticated, command-inaccessible control
channel, signed helper packaging, operation/session fencing and crash recovery.
The guardian must be outside the worker's group; it establishes its own session
unless the caller already did so. Commands have no interactive stdin in this mode.

This is process ownership, not a security boundary. A command can escape its group
or attack same-user processes. Killing the guardian itself also defeats cleanup.
Untrusted shared-machine tasks still require VM/container isolation, bounded
resources and secret/network separation. Windows uses its existing Job Object
path instead of this Unix helper.
