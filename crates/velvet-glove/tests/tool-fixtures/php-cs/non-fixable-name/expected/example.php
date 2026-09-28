<?php

namespace Demo;

class Greeter
{
    public function say_hello(string $name): string
    {
        return "hello, " . $name;
    }
}
