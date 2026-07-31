// @noLib: true

namespace Outside {
    function implemented(): void {}
    function signature(): void;
    class PlainClass {}
}

namespace Inside {
    function bodyError(): void {
        const value = ;
    }
    class classError {
        value = ;
    }
}
