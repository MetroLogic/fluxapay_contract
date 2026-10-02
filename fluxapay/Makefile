default: build

all: test

test: build
	cargo test

build:
	stellar contract build
	@ls -l target/wasm32-unknown-unknown/release/*.wasm

# Build and optimize the WASM contract
# Uses stellar contract optimize which applies wasm-opt -O3 and other optimizations
optimize:
	stellar contract build
	stellar contract optimize --wasm target/wasm32-unknown-unknown/release/fluxapay.wasm
	@echo "Optimized WASM size:"
	@ls -lh target/wasm32-unknown-unknown/release/*.wasm
	@echo ""
	@echo "Checking against MAX_WASM_SIZE_KB=100 limit..."
	@MAX_SIZE=$$((100 * 1024)); \
	for wasm in target/wasm32-unknown-unknown/release/*.wasm; do \
		size=$$(stat -c%s "$$wasm"); \
		name=$$(basename "$$wasm"); \
		if [ "$$size" -gt "$$MAX_SIZE" ]; then \
			echo "❌ $$name: $$size bytes (exceeds $$MAX_SIZE bytes limit)"; \
		else \
			echo "✅ $$name: $$size bytes (within limit)"; \
		fi; \
	done

fmt:
	cargo fmt --all

clean:
	cargo clean