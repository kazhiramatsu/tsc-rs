/*---------------------------------
 *  Copyright header.
 *--------------------------------*/

import { deco } from './dep.js';

@deco
export class Self { static make() { return new Self(); } }
