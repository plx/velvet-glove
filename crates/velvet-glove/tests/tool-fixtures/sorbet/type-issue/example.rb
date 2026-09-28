# typed: true
class Foo
  extend T::Sig

  sig { returns(Integer) }
  def bar
    "not an int"
  end
end
