INSTALL_DIR := $(HOME)/.local/bin
BUILD_DIR := build
RUSTC := rustc
RELEASE_FLAGS := -O

BINARIES := $(BUILD_DIR)/fixtex $(BUILD_DIR)/fixbib

.PHONY: all install uninstall clean

all: $(BINARIES)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/fixtex: fixtex.rs | $(BUILD_DIR)
	$(RUSTC) $(RELEASE_FLAGS) -o $@ $<

$(BUILD_DIR)/fixbib: fixbib.rs | $(BUILD_DIR)
	$(RUSTC) $(RELEASE_FLAGS) -o $@ $<

install: all
	mkdir -p $(INSTALL_DIR)
	cp -f $(BUILD_DIR)/fixtex $(BUILD_DIR)/fixbib $(INSTALL_DIR)/
	chmod +x $(INSTALL_DIR)/fixtex $(INSTALL_DIR)/fixbib
	@echo "Installed fixtex and fixbib to $(INSTALL_DIR)"
	@echo "Make sure $(INSTALL_DIR) is in your PATH."

uninstall:
	rm -f $(INSTALL_DIR)/fixtex $(INSTALL_DIR)/fixbib
	@echo "Uninstalled fixtex and fixbib from $(INSTALL_DIR)"

clean:
	rm -rf $(BUILD_DIR)
