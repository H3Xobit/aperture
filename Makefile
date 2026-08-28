.PHONY: test cpp rust python run compose-up compose-down

test: cpp rust python

cpp:
	$(MAKE) -C cpp/limiter test

rust:
	cargo test --workspace

python:
	cd python/embed && python3 -m unittest test_chunker.py -q

run:
	APERTURE_WEB=web cargo run -p aperture-gateway

compose-up:
	docker compose up -d

compose-down:
	docker compose down
