set -eu
T=/opt/hive-control-pilot/reconnect-test
B=$T/runtime/usr/lib/postgresql/16/bin
mkdir -p "$T/data" "$T/socket"
chown nobody:nogroup "$T/data" "$T/socket"
runuser -u nobody -- env LD_LIBRARY_PATH="$T/runtime/usr/lib/x86_64-linux-gnu" "$B/initdb" -D "$T/data" -L "$T/runtime/usr/share/postgresql/16" -U sif_test --auth=trust > "$T/initdb.log"
openssl req -x509 -newkey rsa:2048 -nodes -keyout "$T/data/server.key" -out "$T/data/server.crt" -days 1 -subj /CN=localhost -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' > "$T/cert.log" 2>&1
chown nobody:nogroup "$T/data/server.key" "$T/data/server.crt"
chmod 600 "$T/data/server.key"
cat >> "$T/data/postgresql.conf" <<EOF
listen_addresses = '127.0.0.1'
port = 55439
unix_socket_directories = '$T/socket'
ssl = on
ssl_cert_file = '$T/data/server.crt'
ssl_key_file = '$T/data/server.key'
dynamic_library_path = '$T/runtime/usr/lib/postgresql/16/lib'
EOF
runuser -u nobody -- env LD_LIBRARY_PATH="$T/runtime/usr/lib/x86_64-linux-gnu" "$B/pg_ctl" -D "$T/data" -l "$T/data/postgres.log" start
LD_LIBRARY_PATH="$T/runtime/usr/lib/x86_64-linux-gnu" "$B/psql" -h 127.0.0.1 -p 55439 -U sif_test -d postgres -c 'create schema hive; create function hive.ctl_pilot_ready() returns boolean language sql as $$ select true $$;'
