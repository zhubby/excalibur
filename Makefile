SHELL := /bin/bash

COMPOSE ?= docker compose
CARGO ?= cargo
BUN ?= bun
RUSTUP_TOOLCHAIN ?= stable

API_ADDR ?= 0.0.0.0:8080
API_URL ?= http://localhost:8080
NEXT_PUBLIC_API_BASE_URL ?= $(API_URL)
MQTT_LISTEN ?= 0.0.0.0:1883
MQTT_URL ?= mqtt://localhost:1883
DEVICE_MQTT_BROKER ?= localhost
DEVICE_MQTT_PORT ?= 1883

STORAGE_BACKEND ?= timescale
DATABASE_URL ?= postgres://excalibur:excalibur@localhost:5432/excalibur
NATS_URL ?= nats://localhost:4222
S3_ENDPOINT ?= http://localhost:9000

export RUSTUP_TOOLCHAIN

.DEFAULT_GOAL := help

.PHONY: help setup frontend-deps infra backend backend-memory mqtt mqtt-memory frontend dev dev-full dev-memory stack stop logs health check

help: ## Show available commands.
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make <target>\n\nTargets:\n"} /^[a-zA-Z0-9_.-]+:.*##/ {printf "  %-16s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

setup: frontend-deps ## Install local frontend dependencies.

frontend-deps: ## Install frontend dependencies when node_modules is missing.
	@if [ ! -d frontend/node_modules ]; then \
		cd frontend && $(BUN) install; \
	fi

infra: ## Start local TimescaleDB, NATS, and RustFS.
	$(COMPOSE) up -d timescaledb nats rustfs

backend: infra ## Start the SQL-backed Axum API on API_ADDR.
	cd backend && \
	API_ADDR="$(API_ADDR)" \
	STORAGE_BACKEND="$(STORAGE_BACKEND)" \
	DATABASE_URL="$(DATABASE_URL)" \
	NATS_URL="$(NATS_URL)" \
	S3_ENDPOINT="$(S3_ENDPOINT)" \
	DEVICE_MQTT_BROKER="$(DEVICE_MQTT_BROKER)" \
	DEVICE_MQTT_PORT="$(DEVICE_MQTT_PORT)" \
	$(CARGO) run -p excalibur-api

backend-memory: ## Start the Axum API with in-memory storage.
	cd backend && \
	API_ADDR="$(API_ADDR)" \
	STORAGE_BACKEND=memory \
	DEVICE_MQTT_BROKER="$(DEVICE_MQTT_BROKER)" \
	DEVICE_MQTT_PORT="$(DEVICE_MQTT_PORT)" \
	$(CARGO) run -p excalibur-api

mqtt: infra ## Start the SQL-backed rumqttd MQTT broker and ingest runtime.
	cd backend && \
	MQTT_LISTEN="$(MQTT_LISTEN)" \
	STORAGE_BACKEND="$(STORAGE_BACKEND)" \
	DATABASE_URL="$(DATABASE_URL)" \
	NATS_URL="$(NATS_URL)" \
	$(CARGO) run -p excalibur-mqtt-ingest

mqtt-memory: ## Start the rumqttd MQTT broker with in-memory ingest storage.
	cd backend && \
	MQTT_LISTEN="$(MQTT_LISTEN)" \
	STORAGE_BACKEND=memory \
	$(CARGO) run -p excalibur-mqtt-ingest

frontend: frontend-deps ## Start the Next.js console on http://localhost:3000.
	cd frontend && \
	NEXT_PUBLIC_API_BASE_URL="$(NEXT_PUBLIC_API_BASE_URL)" \
	$(BUN) run dev

dev: infra frontend-deps ## Start SQL-backed backend and frontend together.
	@echo "Starting Excalibur API at $(API_URL)"
	@echo "Starting Excalibur Console at http://localhost:3000"
	@set -euo pipefail; \
	api_pid=""; \
	frontend_pid=""; \
	cleanup() { \
		status=$$?; \
		if [ -n "$$api_pid" ]; then kill "$$api_pid" 2>/dev/null || true; fi; \
		if [ -n "$$frontend_pid" ]; then kill "$$frontend_pid" 2>/dev/null || true; fi; \
		wait "$$api_pid" "$$frontend_pid" 2>/dev/null || true; \
		exit "$$status"; \
	}; \
	trap cleanup INT TERM EXIT; \
	( \
		cd backend && \
		API_ADDR="$(API_ADDR)" \
		STORAGE_BACKEND="$(STORAGE_BACKEND)" \
		DATABASE_URL="$(DATABASE_URL)" \
		NATS_URL="$(NATS_URL)" \
		S3_ENDPOINT="$(S3_ENDPOINT)" \
		DEVICE_MQTT_BROKER="$(DEVICE_MQTT_BROKER)" \
		DEVICE_MQTT_PORT="$(DEVICE_MQTT_PORT)" \
		$(CARGO) run -p excalibur-api \
	) & \
	api_pid=$$!; \
	( \
		cd frontend && \
		NEXT_PUBLIC_API_BASE_URL="$(NEXT_PUBLIC_API_BASE_URL)" \
		$(BUN) run dev \
	) & \
	frontend_pid=$$!; \
	wait "$$api_pid" "$$frontend_pid"

dev-full: infra frontend-deps ## Start SQL-backed API, MQTT broker, and frontend together.
	@echo "Starting Excalibur API at $(API_URL)"
	@echo "Starting Excalibur MQTT broker at $(MQTT_URL)"
	@echo "Starting Excalibur Console at http://localhost:3000"
	@set -euo pipefail; \
	api_pid=""; \
	mqtt_pid=""; \
	frontend_pid=""; \
	cleanup() { \
		status=$$?; \
		if [ -n "$$api_pid" ]; then kill "$$api_pid" 2>/dev/null || true; fi; \
		if [ -n "$$mqtt_pid" ]; then kill "$$mqtt_pid" 2>/dev/null || true; fi; \
		if [ -n "$$frontend_pid" ]; then kill "$$frontend_pid" 2>/dev/null || true; fi; \
		wait "$$api_pid" "$$mqtt_pid" "$$frontend_pid" 2>/dev/null || true; \
		exit "$$status"; \
	}; \
	trap cleanup INT TERM EXIT; \
	( \
		cd backend && \
		API_ADDR="$(API_ADDR)" \
		STORAGE_BACKEND="$(STORAGE_BACKEND)" \
		DATABASE_URL="$(DATABASE_URL)" \
		NATS_URL="$(NATS_URL)" \
		S3_ENDPOINT="$(S3_ENDPOINT)" \
		DEVICE_MQTT_BROKER="$(DEVICE_MQTT_BROKER)" \
		DEVICE_MQTT_PORT="$(DEVICE_MQTT_PORT)" \
		$(CARGO) run -p excalibur-api \
	) & \
	api_pid=$$!; \
	( \
		cd backend && \
		MQTT_LISTEN="$(MQTT_LISTEN)" \
		STORAGE_BACKEND="$(STORAGE_BACKEND)" \
		DATABASE_URL="$(DATABASE_URL)" \
		NATS_URL="$(NATS_URL)" \
		$(CARGO) run -p excalibur-mqtt-ingest \
	) & \
	mqtt_pid=$$!; \
	( \
		cd frontend && \
		NEXT_PUBLIC_API_BASE_URL="$(NEXT_PUBLIC_API_BASE_URL)" \
		$(BUN) run dev \
	) & \
	frontend_pid=$$!; \
	wait "$$api_pid" "$$mqtt_pid" "$$frontend_pid"

dev-memory: frontend-deps ## Start in-memory backend and frontend together without Docker infra.
	@echo "Starting Excalibur API at $(API_URL) with in-memory storage"
	@echo "Starting Excalibur Console at http://localhost:3000"
	@set -euo pipefail; \
	api_pid=""; \
	frontend_pid=""; \
	cleanup() { \
		status=$$?; \
		if [ -n "$$api_pid" ]; then kill "$$api_pid" 2>/dev/null || true; fi; \
		if [ -n "$$frontend_pid" ]; then kill "$$frontend_pid" 2>/dev/null || true; fi; \
		wait "$$api_pid" "$$frontend_pid" 2>/dev/null || true; \
		exit "$$status"; \
	}; \
	trap cleanup INT TERM EXIT; \
	( \
		cd backend && \
		API_ADDR="$(API_ADDR)" \
		STORAGE_BACKEND=memory \
		DEVICE_MQTT_BROKER="$(DEVICE_MQTT_BROKER)" \
		DEVICE_MQTT_PORT="$(DEVICE_MQTT_PORT)" \
		$(CARGO) run -p excalibur-api \
	) & \
	api_pid=$$!; \
	( \
		cd frontend && \
		NEXT_PUBLIC_API_BASE_URL="$(NEXT_PUBLIC_API_BASE_URL)" \
		$(BUN) run dev \
	) & \
	frontend_pid=$$!; \
	wait "$$api_pid" "$$frontend_pid"

stack: ## Start the Docker Compose app stack with rebuilt images.
	$(COMPOSE) up --build api mqtt-ingest worker frontend

stop: ## Stop local Docker Compose services.
	$(COMPOSE) down

logs: ## Follow Docker Compose logs.
	$(COMPOSE) logs -f

health: ## Check the local API health endpoint.
	curl -fsS "$(API_URL)/health"

check: ## Run backend and frontend checks.
	cd backend && $(CARGO) fmt --all --check
	cd backend && $(CARGO) check --workspace
	cd frontend && $(BUN) run typecheck
	cd frontend && $(BUN) run test
