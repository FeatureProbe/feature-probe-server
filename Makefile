build_date = `date +%Y%m%d%H%M`
commit = `git rev-parse HEAD`
version = `git rev-parse --short HEAD`

.PHONY: release
release:
	 (ssh-agent -k || true) && \
		eval `ssh-agent -s` && \
		ssh-add && \
		cargo build --release --verbose
test:
	 (ssh-agent -k || true) && \
		eval `ssh-agent -s` && \
		ssh-add && \
		cargo test --release --verbose
example:
	 (ssh-agent -k || true) && \
		eval `ssh-agent -s` && \
		ssh-add && \
		cargo build --examples --release --verbose