class SwitchArrowExpr {
    int f(int x, int y) {
        return switch (x) {
            case 1 -> switch (y) {
                case 0 -> 10;
                default -> 20;
            };
            case 2 -> switch (y) {
                default -> "ab";
            }.length();
            default -> 0;
        };
    }

    void s(int x, int y) {
        switch (x) {
            case 1 -> switch (y) {
                case 0 -> a();
                default -> b();
            };
            default -> c();
        }
    }
}
