//! A service to proxy Binder requests between VMs.

use binder::{
    add_service, get_service, Binder, Interface, InterfaceClass, Parcel, Remotable, TransactionCode,
};

struct Service {}

impl Remotable for Service {
    fn get_descriptor() -> &'static str {
        "test_service_descriptor"
    }

    fn on_transact(
        &self,
        code: TransactionCode,
        _data: &Parcel,
        _reply: &mut Parcel,
    ) -> binder::Result<()> {
        println!("on_transact({:?})", code);
        Ok(())
    }

    fn get_class() -> InterfaceClass {
        InterfaceClass::new::<Binder<Service>>()
    }
}

fn main() {
    println!("Hello, world!");

    let service = Service {};
    let binder = Binder::new(service);
    let result = add_service("my_service", binder.as_binder());
    println!("add_service={:?}", result);

    let service = get_service("service_name");
    println!("service={:#?}", service);
}
