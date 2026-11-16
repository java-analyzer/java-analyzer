@interface Config {
    String name() default "x";
    int[] values() default {1, 2, 3};
    Class<?> type() default Object.class;
    Level level() default Level.INFO;
    String[] tags();
}
