use std::cell::RefCell;

type Handler<T> = Box<dyn Fn(&T) -> bool>;

pub struct Signal<T> {
    handlers: RefCell<Vec<Handler<T>>>,
}

impl<T> Default for Signal<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Signal<T> {
    pub const fn new() -> Self {
        Signal { handlers: RefCell::new(Vec::new()) }
    }

    pub fn connect(&self, f: impl Fn(&T) -> bool + 'static) {
        self.handlers.borrow_mut().push(Box::new(f));
    }

    pub fn emit(&self, value: &T) {
        let current = std::mem::take(&mut *self.handlers.borrow_mut());
        let mut kept: Vec<Handler<T>> = Vec::with_capacity(current.len());
        for h in current {
            if h(value) {
                kept.push(h);
            }
        }
        let added = std::mem::take(&mut *self.handlers.borrow_mut());
        kept.extend(added);
        *self.handlers.borrow_mut() = kept;
    }

    pub fn len(&self) -> usize {
        self.handlers.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
