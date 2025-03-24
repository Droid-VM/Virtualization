package com.ferrochrome;

import com.ferrochrome.IInstance;

interface IService {
    boolean ping();
    IInstance create();
}
