.PHONY: help deps dev run clean deb test release

PREFIX ?= /usr/local
CARGO  ?= cargo
BINARY := shred-graphique

GREEN  := \033[0;32m
YELLOW := \033[1;33m
NC     := \033[0m

help: ## Affiche l'aide
	@echo "$(GREEN)Shred Graphique$(NC)"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  $(YELLOW)%-12s$(NC) %s\n", $$1, $$2}'
	@echo ""
	@echo "PREFIX=$(PREFIX)  (installation dev: make dev)"

deps: ## Installe et verifie les dependances systeme (apt)
	@./scripts/deps.sh

dev: deps ## Build release + installation locale (helper + polkit)
	@$(CARGO) build --release
	@PREFIX="$(PREFIX)" ./scripts/install-dev.sh

run: ## Lance l'application (apres make dev)
	@SHRED_GRAPHIQUE_HELPER="$(PREFIX)/bin/shred-graphique-helper" \
		"$(PREFIX)/bin/$(BINARY)"

release: ## Compile en mode release uniquement
	@$(CARGO) build --release

clean: ## Nettoie les artefacts de build
	@$(CARGO) clean

deb: ## Construit un paquet .deb
	@./packaging/build_deb.sh

test: ## Lance les tests unitaires
	@$(CARGO) test

.DEFAULT_GOAL := help
