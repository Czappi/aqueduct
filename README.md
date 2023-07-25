# Aqueduct

Magic-level wrapper around [flutter_rust_bridge](https://github.com/fzyzcjy/flutter_rust_bridge).
Main goal is to replace the rust codegen with macros.

## :construction: Initialization

### For applications

```rust
// main.rs

// This is needed for [TypeDescriptor] collection
aqueduct::init!(external_lib::TYPE_DESCRIPTORS, other_external_lib::TYPE_DESCRIPTORS);

// If there are external aqueduct libraries, then those can be imported like this
// and those types generated as well
//
// Still can't have name conflict!
aqueduct::load_lib!(external_lib);
aqueduct::load_lib!(other_external_lib);

fn main() {
...
}
```

```rust
// build.rs

fn main() {
    // Generate dart files to folder
    // dart/flutter project folder for generated dart files
    aqueduct::compile!("../lib/generated")
        // Folder for generated definition file(s)
        // defaults to compile folder
        .dart_definition_folder("../lib/generated/definitions")

        // `DartDefinition::Separate` (separate types to invidiual file)
        // 'DartDefinition::SingleFile("filename")' (single definition file containing every type)
        .dart_definition(DartDefinition::Separate)

        // Generate WASM binding (when implemented)
        // defaults to `true`
        .wasm(false)
        .build()
}

```


### For libraries

```rust
// lib.rs

// This is needed for [TypeDescriptor] collection
aqueduct::init!();

fn main() {
...
}
```

## :construction: Derive macro
```rust
#[derive(aqueduct::Wire)]
pub struct ExampleStruct {
    text: String,
    other: OtherStruct
}
```

## :construction: Manual implementation

For external type wrappers.

```rust
pub struct ExampleStruct {
    text: String,
    other: OtherStruct // this have the `aqueduct::Wire` derive macro too 
}

impl ExampleStruct {
    pub fn example_method(method_text: String) -> u64 {
        ...
    }
}

// the type have to implement [TypeDescription]
// if something wrong (type name conflict) this will throw a compiler error!
impl_wire!(ExampleStruct)

impl TypeDescription for ExampleStruct {

    fn type_descriptor() -> DartDescriptor {
        // every type here have to implement the [Wire] trait
        TypeDescriptor::struct("ExampleStruct") // this have to be the exact same as the struct name
            .field::<String>("text") // these have to be the exact same as the fields
            .field::<OtherStruct>("other")
            // future feature?
            .method(
                TypeDescriptor::function::<u64>::("example_method") // this have to be the exact same as the method name
                    .parameter::<String>("method_text") // these have to be the exact same as the parameters
            )
    }
}
```

## :construction: Function
```rust
// This function can be called from Dart/Flutter side
// Function called in an async executer (tokio)
#[aqueduct::function]
fn simple_adder(x: u32, y: u32) -> {
    x + y
}
```
