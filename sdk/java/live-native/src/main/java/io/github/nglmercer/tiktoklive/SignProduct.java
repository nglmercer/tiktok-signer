package io.github.nglmercer.tiktoklive;

public enum SignProduct {
    FETCH("fetch"),
    FRONTIER("frontier"),
    WS("ws");
    private final String wire;

    SignProduct(String wire) {
        this.wire = wire;
    }

    String wire() {
        return wire;
    }
}
