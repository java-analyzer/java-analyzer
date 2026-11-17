class TypeAnnotations<@A T extends @B Object> extends @A Base<@B String>
    implements @A List<@B ? extends @C Number>, @D List<@E ?> {
  Outer.@A Inner<@B String> nested;
  ThreadLocal<int @A []> primArray;
  char[] @A [] multiArray1;
  String @A [] @B [] multiArray2;
  int legacyDims @A [];

  <@A U extends @B Foo & @C Bar> @D U genericMethod(@E U @F ... args) throws @G Exception {
    List<@Anno("x") String> withArgs = null;
    @A String local = (@B String[][]) args;
    int n = (@A int) 1L;
    Object intersection = (@A Foo & @B Bar) args;
    Object o = new @A Object();
    int[][] a = new int @A [1] @B [];
    int[] b = new int @A [] {1, 2};
    if (o instanceof @A String) {}
    if (o instanceof @A String s) {}
    for (@A String item : withArgs) {}
    try {
    } catch (@A RuntimeException | @B Error e) {
    }
    return (@A U) local;
  }
}
