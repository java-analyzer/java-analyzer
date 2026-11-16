class A {
    Runnable r = new Runnable() { public void run() {} };
    Runnable s = () -> { work(); };
    int after;
}
