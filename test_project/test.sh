cargo run --bin capsules_compiler -- -i capsule.json -t aarch64-apple-darwin

./capsule-aarch64-apple-darwin version
./capsule-aarch64-apple-darwin deamon start
sleep 1
./capsule-aarch64-apple-darwin proc list

./capsule-aarch64-apple-darwin deamon stop
