/*---------------------------------
 *  Copyright header.
 *--------------------------------*/

import { I } from './dep.js';
import { deco } from './dep.js';

@deco
export class G implements I { x = 1; static make() { return new G(); } }
