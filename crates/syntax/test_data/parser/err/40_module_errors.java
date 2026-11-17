module com.example.broken {
    requires ;
    exports com.example.api to ;
    provides Svc impl1;
    bogus directive;
    uses Svc
}
