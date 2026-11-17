import com.example.Service;
import com.example.impl.*;

@Deprecated(since = "21")
open module com.example.app {
    requires java.base;
    requires static java.compiler;
    requires transitive java.logging;
    requires static transitive java.sql;
    requires transitive;
    requires transitive.helper;
    exports com.example.api;
    exports com.example.internal to com.example.test, com.example.tools;
    opens com.example.model;
    opens com.example.secret to com.example.framework;
    uses Service;
    uses com.example.spi.Plugin;
    provides Service with DefaultService, ExtraService;
}
